import { ArrowDown, ArrowLeft, Bookmark, MessageSquare, Paperclip, Reply, X } from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { useMentionPicker } from "../hooks/useMentionPicker";
import { APP_DISPLAY_NAME } from "../branding";
import { isImeComposing } from "../input-utils";
import { copyText } from "../clipboard";
import { isCompactFollowupMessage, wasEdited } from "../message-grouping";
import { messageShareLink, messageToMarkdown } from "../message-share";
import { Agent, AgentActivity, AgentRun, AgentWorkItem, Artifact, Channel, DraftAttachment, Message, OwnerProfile, TASK_STATUSES, Task } from "../types";
import { agentForMessageSender, deletedAgentForMessageSender, formatClockTime, formatTime, ownerAsAvatarAgent, visibleAgentDescription } from "../ui-utils";
import { ActivityProgressDock } from "./ActivityProgressDock";
import { AgentAvatar, AgentAvatarWithProfile } from "./AgentAvatar";
import { DraftAttachmentsPreview } from "./DraftAttachmentsPreview";
import { MessageActionMenu } from "./MessageActionMenu";
import { MessageAttachments } from "./MessageAttachments";
import { MessageArtifacts } from "./MessageArtifacts";
import { MessageMarkdown } from "./MessageMarkdown";
import { TaskAssigneePicker } from "./TaskAssigneePicker";

function taskStatusLabel(status: string) {
  return status.replace("_", " ");
}

type ThreadPanelProps = {
  channel: Channel | null;
  agents: Agent[];
  ownerProfile: OwnerProfile;
  agentActivities: AgentActivity[];
  agentRuns: AgentRun[];
  agentWorkItems: AgentWorkItem[];
  activeRoot: Message | null;
  activeTask: Task | null;
  replies: Message[];
  unreadCount: number;
  taskTitleDrafts: Record<string, string>;
  replyDraft: string;
  replyAttachments: DraftAttachment[];
  onClose: () => void;
  setTaskTitleDraft: (task: Task, title: string) => void;
  saveTaskTitle: (task: Task) => void;
  claimTask: (task: Task, agentId: string) => void;
  updateTaskStatus: (task: Task, status: string) => void;
  setReplyDraft: (value: string) => void;
  addReplyAttachments: (files: FileList | File[]) => void;
  removeReplyAttachment: (id: string) => void;
  sendReply: () => void;
  openAgentDetail: (agent: Agent) => void;
  openArtifact: (artifact: Artifact) => void;
  shareBaseUrl: string | null;
  savedMessageIds: Set<string>;
  focusedMessageId: string | null;
  onToggleMessageSaved: (message: Message, saved: boolean) => void;
  onResizeStart: (event: ReactPointerEvent<HTMLButtonElement>) => void;
};

type MessageMenuState = {
  x: number;
  y: number;
  message: Message;
} | null;

export function ThreadPanel({
  channel,
  agents,
  ownerProfile,
  agentActivities,
  agentRuns,
  agentWorkItems,
  activeRoot,
  activeTask,
  replies,
  unreadCount,
  taskTitleDrafts,
  replyDraft,
  replyAttachments,
  onClose,
  setTaskTitleDraft,
  saveTaskTitle,
  claimTask,
  updateTaskStatus,
  setReplyDraft,
  addReplyAttachments,
  removeReplyAttachment,
  sendReply,
  openAgentDetail,
  openArtifact,
  shareBaseUrl,
  savedMessageIds,
  focusedMessageId,
  onToggleMessageSaved,
  onResizeStart,
}: ThreadPanelProps) {
  const [isReplyDragOver, setIsReplyDragOver] = useState(false);
  const [showBackToBottom, setShowBackToBottom] = useState(false);
  const [messageMenu, setMessageMenu] = useState<MessageMenuState>(null);
  const [tapFocusedMessageId, setTapFocusedMessageId] = useState<string | null>(null);
  const replyDragDepthRef = useRef(0);
  const longPressTimerRef = useRef<number | null>(null);
  const threadScrollRef = useRef<HTMLDivElement | null>(null);
  const shouldFollowThreadRef = useRef(true);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const isDm = channel?.kind === "dm";
  const dmAgent = isDm ? agents.find((agent) => agent.id === channel?.dm_agent_id) ?? null : null;
  const rootAgent = activeRoot ? agentForMessageSender(activeRoot, agents) : null;
  const deletedRootAgent = activeRoot && !rootAgent ? deletedAgentForMessageSender(activeRoot) : null;
  const rootSaved = activeRoot ? savedMessageIds.has(activeRoot.id) : false;
  const surfaceLabel = channel
    ? isDm
      ? `Thread in DM with @${dmAgent?.handle || "agent"}`
      : `Thread in #${channel.name}`
    : `${APP_DISPLAY_NAME} thread`;
  const {
    mentionState,
    mentionIndex,
    mentionCandidates,
    refreshMentionState,
    chooseMention,
    handleMentionKeyDown,
    closeMentionPicker,
    focusComposer,
  } = useMentionPicker({ agents, value: replyDraft, setValue: setReplyDraft, textareaRef });
  const lastReply = replies[replies.length - 1] ?? null;

  function isThreadScrollAtBottom(element: HTMLDivElement) {
    return element.scrollHeight - element.scrollTop - element.clientHeight < 32;
  }

  function scrollThreadToBottom(behavior: ScrollBehavior = "auto") {
    const element = threadScrollRef.current;
    if (!element) return;
    element.scrollTo({ top: element.scrollHeight, behavior });
  }

  function handleThreadScroll() {
    const element = threadScrollRef.current;
    if (!element) return;
    const atBottom = isThreadScrollAtBottom(element);
    shouldFollowThreadRef.current = atBottom;
    const shouldShow = Boolean(activeRoot) && !atBottom;
    setShowBackToBottom((current) => current === shouldShow ? current : shouldShow);
  }

  function handleThreadContentLoad() {
    if (!shouldFollowThreadRef.current) return;
    scrollThreadToBottom();
  }

  function returnThreadToBottom() {
    shouldFollowThreadRef.current = true;
    setShowBackToBottom(false);
    scrollThreadToBottom();
    window.requestAnimationFrame(() => {
      scrollThreadToBottom();
      shouldFollowThreadRef.current = true;
      setShowBackToBottom(false);
    });
  }

  function handleReplyKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (isImeComposing(event)) return;
    if (handleMentionKeyDown(event)) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submitReply();
    }
  }

  function submitReply() {
    if (!activeRoot || (!replyDraft.trim() && replyAttachments.length === 0)) return;
    sendReply();
    closeMentionPicker();
    focusComposer();
  }

  function hasDraggedFiles(event: DragEvent<HTMLElement>) {
    return Array.from(event.dataTransfer.types).includes("Files");
  }

  function handleReplyDragEnter(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    replyDragDepthRef.current += 1;
    event.dataTransfer.dropEffect = activeRoot ? "copy" : "none";
    if (activeRoot) setIsReplyDragOver(true);
  }

  function handleReplyDragOver(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = activeRoot ? "copy" : "none";
    if (activeRoot) setIsReplyDragOver(true);
  }

  function handleReplyDragLeave(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    replyDragDepthRef.current = Math.max(0, replyDragDepthRef.current - 1);
    if (replyDragDepthRef.current === 0) setIsReplyDragOver(false);
  }

  function handleReplyDrop(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    replyDragDepthRef.current = 0;
    setIsReplyDragOver(false);
    if (!activeRoot || event.dataTransfer.files.length === 0) return;
    addReplyAttachments(event.dataTransfer.files);
    focusComposer();
  }

  function handleReplyPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const imageFiles = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
    if (imageFiles.length === 0) return;
    event.preventDefault();
    if (!activeRoot) return;
    addReplyAttachments(imageFiles);
    focusComposer();
  }

  useEffect(() => {
    replyDragDepthRef.current = 0;
    setIsReplyDragOver(false);
    setMessageMenu(null);
    setTapFocusedMessageId(null);
  }, [activeRoot?.id]);

  useLayoutEffect(() => {
    shouldFollowThreadRef.current = true;
    setShowBackToBottom(false);
    scrollThreadToBottom();
  }, [activeRoot?.id]);

  useLayoutEffect(() => {
    if (!shouldFollowThreadRef.current) return;
    scrollThreadToBottom();
  }, [activeRoot?.id, activeRoot?.updated_at, replies.length, lastReply?.id, lastReply?.updated_at, lastReply?.delivery_state]);

  useEffect(() => {
    if (!focusedMessageId) return;
    const element = threadScrollRef.current?.querySelector<HTMLElement>(`[data-message-id="${focusedMessageId}"]`);
    element?.scrollIntoView({ block: "center" });
  }, [activeRoot?.id, focusedMessageId, replies.length]);

  useEffect(() => clearLongPress, []);

  function clearLongPress() {
    if (longPressTimerRef.current === null) return;
    window.clearTimeout(longPressTimerRef.current);
    longPressTimerRef.current = null;
  }

  function startMessageLongPress(event: ReactPointerEvent<HTMLElement>, message: Message) {
    if (event.pointerType === "mouse") return;
    clearLongPress();
    const x = event.clientX;
    const y = event.clientY;
    longPressTimerRef.current = window.setTimeout(() => {
      setMessageMenu({ x, y, message });
      longPressTimerRef.current = null;
    }, 520);
  }

  async function copyMessageMarkdown(message: Message) {
    await copyText(messageToMarkdown(message, surfaceLabel));
    setMessageMenu(null);
  }

  async function copyMessageLink(message: Message) {
    await copyText(messageShareLink(message, shareBaseUrl));
    setMessageMenu(null);
  }

  const activeTaskAssignee = activeTask
    ? agents.find((agent) => agent.id === activeTask.assignee_id) ?? null
    : null;

  return (
    <aside className="thread">
      <button
        className="thread-resize-handle"
        aria-label="Resize thread panel"
        onPointerDown={onResizeStart}
      />
      <header>
        <button
          type="button"
          className="thread-mobile-back"
          onClick={onClose}
          aria-label={isDm ? "Back to direct message" : "Back to channel"}
        >
          <ArrowLeft size={18} />
        </button>
        <div>
          <h2>
            Thread <span>{channel ? isDm ? `- @${dmAgent?.handle || "agent"}` : `- #${channel.name}` : "- no channel"}</span>
          </h2>
          <p>
            {activeRoot ? `${replies.length} ${replies.length === 1 ? "reply" : "replies"}` : "No thread selected"}
            {unreadCount > 0 ? ` · ${unreadCount} new` : ""}
          </p>
        </div>
        <button type="button" className="thread-close" onClick={onClose} aria-label="Close thread panel"><X size={18} /></button>
      </header>

      <section className="thread-focus">
        <div
          ref={threadScrollRef}
          className="thread-scroll"
          onScroll={handleThreadScroll}
          onLoadCapture={handleThreadContentLoad}
        >
          <ActivityProgressDock
            messages={replies}
            activities={agentActivities}
            runs={agentRuns}
            workItems={agentWorkItems}
            agents={agents}
            channelId={activeRoot ? channel?.id ?? null : null}
            threadRootId={activeRoot?.id ?? null}
          />
          {activeRoot && (
            <article
              data-message-id={activeRoot.id}
              className={`thread-root ${activeRoot.sender_role === "system" ? "system-message" : ""} ${tapFocusedMessageId === activeRoot.id ? "tap-focused" : ""} ${rootSaved ? "saved" : ""}`}
              data-jump-focused={focusedMessageId === activeRoot.id ? "true" : "false"}
              onClick={() => {
                if (activeRoot.sender_role !== "system") setTapFocusedMessageId(activeRoot.id);
              }}
              onContextMenu={(event) => {
                if (activeRoot.sender_role === "system") return;
                event.preventDefault();
                setMessageMenu({ x: event.clientX, y: event.clientY, message: activeRoot });
              }}
              onPointerDown={(event) => {
                if (activeRoot.sender_role !== "system") {
                  setTapFocusedMessageId(activeRoot.id);
                  startMessageLongPress(event, activeRoot);
                }
              }}
              onPointerMove={clearLongPress}
              onPointerUp={clearLongPress}
              onPointerCancel={clearLongPress}
              onPointerLeave={clearLongPress}
            >
              {activeRoot.sender_role === "system" ? (
                <div className="system-message-line">
                  <MessageMarkdown body={activeRoot.body} />
                  <time>{formatTime(activeRoot.created_at)}</time>
                </div>
              ) : (
                <div className="thread-message-with-avatar">
                  {rootAgent ? (
                    <button
                      type="button"
                      className="message-agent-avatar-trigger"
                      aria-label={`View @${rootAgent.handle} details`}
                      onClick={(event) => {
                        event.stopPropagation();
                        openAgentDetail(rootAgent);
                      }}
                    >
                      <AgentAvatarWithProfile agent={rootAgent} />
                    </button>
                  ) : deletedRootAgent ? (
                    <AgentAvatar
                      agent={deletedRootAgent}
                      size="md"
                      title={`@${deletedRootAgent.handle} has been deleted`}
                    />
                  ) : activeRoot.sender_role === "owner" ? (
                    <AgentAvatar agent={ownerAsAvatarAgent(ownerProfile)} size="md" showStatus={false} />
                  ) : (
                    <div className="avatar">{activeRoot.sender_name.slice(0, 1)}</div>
                  )}
                  <div className="thread-message-content">
                    <div className="meta">
                      <strong>{activeRoot.sender_name}</strong>
                      <time>{formatTime(activeRoot.created_at)}</time>
                      {wasEdited(activeRoot) && <span className="edited-indicator">edited</span>}
                      <button
                        type="button"
                        className={`message-save-button ${rootSaved ? "saved" : ""}`}
                        title={rootSaved ? "Unsave message" : "Save message"}
                        onPointerDown={(event) => event.stopPropagation()}
                        onClick={(event) => {
                          event.stopPropagation();
                          onToggleMessageSaved(activeRoot, !rootSaved);
                        }}
                      >
                        <Bookmark size={13} />
                        {rootSaved ? "Saved" : "Save"}
                      </button>
                    </div>
                    {activeRoot.delivery_state !== "streaming" && <MessageMarkdown body={activeRoot.body} />}
                    <MessageAttachments attachments={activeRoot.attachments} />
                    <MessageArtifacts artifacts={activeRoot.artifacts} onOpenArtifact={openArtifact} />
                    {activeRoot.delivery_state === "error" && (
                      <div className="message-stream-state error">Response interrupted</div>
                    )}
                  </div>
                </div>
              )}
            </article>
          )}

          {activeTask && (
            <section className="thread-task-card">
              <div className="task-card-head">
                <span>Task #{activeTask.number}</span>
                <strong>{taskStatusLabel(activeTask.status)}</strong>
              </div>
              <input
                value={taskTitleDrafts[activeTask.id] ?? activeTask.title}
                onChange={(event) => setTaskTitleDraft(activeTask, event.target.value)}
                onBlur={() => saveTaskTitle(activeTask)}
                onKeyDown={(event) => {
                  if (isImeComposing(event)) return;
                  if (event.key === "Enter") saveTaskTitle(activeTask);
                }}
              />
              <TaskAssigneePicker
                agents={agents}
                assignee={activeTaskAssignee}
                disabled={activeTask.status === "done"}
                done={activeTask.status === "done"}
                onChange={(agentId) => claimTask(activeTask, agentId)}
                taskNumber={activeTask.number}
              />
              <div className="status-row">
                {TASK_STATUSES.map((status) => (
                  <button
                    type="button"
                    key={status}
                    className={activeTask.status === status ? "active" : ""}
                    data-state={status}
                    onClick={() => updateTaskStatus(activeTask, status)}
                  >
                    {taskStatusLabel(status)}
                  </button>
                ))}
              </div>
            </section>
          )}

          {activeRoot && (
            <div className="thread-replies-divider" aria-label="Beginning of replies">
              <span />
              <div>
                <strong>Beginning of replies</strong>
                <small>{replies.length} {replies.length === 1 ? "reply" : "replies"}</small>
              </div>
              <span />
            </div>
          )}

          <section className="reply-list">
            {!activeRoot && (
              <div className="empty-state compact">
                <MessageSquare size={28} />
                <h2>No thread selected</h2>
                <p>Select a root message after you create one.</p>
              </div>
            )}
            {replies.map((reply, index) => {
              const replyAgent = agentForMessageSender(reply, agents);
              const deletedReplyAgent = replyAgent ? null : deletedAgentForMessageSender(reply);
              const replySaved = savedMessageIds.has(reply.id);
              const isCompact = isCompactFollowupMessage(reply, replies[index - 1]);
              if (reply.sender_role === "system") {
                return (
                  <article key={reply.id} className="system-message">
                    <div className="system-message-line">
                      <MessageMarkdown body={reply.body} />
                      <time>{formatTime(reply.created_at)}</time>
                    </div>
                  </article>
                );
              }
              return (
                <article
                  key={reply.id}
                  data-message-id={reply.id}
                  className={`${isCompact ? "compact" : ""} ${replySaved ? "saved" : ""} ${tapFocusedMessageId === reply.id ? "tap-focused" : ""}`}
                  data-jump-focused={focusedMessageId === reply.id ? "true" : "false"}
                  onClick={() => setTapFocusedMessageId(reply.id)}
                  onContextMenu={(event) => {
                    event.preventDefault();
                    setMessageMenu({ x: event.clientX, y: event.clientY, message: reply });
                  }}
                  onPointerDown={(event) => {
                    setTapFocusedMessageId(reply.id);
                    startMessageLongPress(event, reply);
                  }}
                  onPointerMove={clearLongPress}
                  onPointerUp={clearLongPress}
                  onPointerCancel={clearLongPress}
                  onPointerLeave={clearLongPress}
                >
                  {isCompact ? (
                    <time className="message-compact-time" dateTime={reply.created_at}>
                      {formatClockTime(reply.created_at)}
                    </time>
                  ) : replyAgent ? (
                    <button
                      type="button"
                      className="message-agent-avatar-trigger"
                      aria-label={`View @${replyAgent.handle} details`}
                      onClick={(event) => {
                        event.stopPropagation();
                        openAgentDetail(replyAgent);
                      }}
                    >
                      <AgentAvatarWithProfile agent={replyAgent} />
                    </button>
                  ) : deletedReplyAgent ? (
                    <AgentAvatar
                      agent={deletedReplyAgent}
                      size="md"
                      title={`@${deletedReplyAgent.handle} has been deleted`}
                    />
                  ) : reply.sender_role === "owner" ? (
                    <AgentAvatar agent={ownerAsAvatarAgent(ownerProfile)} size="md" showStatus={false} />
                  ) : (
                    <div className="avatar">{reply.sender_name.slice(0, 1)}</div>
                  )}
                  <div className="reply-body">
                    {!isCompact && (
                      <div className="meta">
                        <strong>{reply.sender_name}</strong>
                        <time>{formatTime(reply.created_at)}</time>
                        {wasEdited(reply) && <span className="edited-indicator">edited</span>}
                        <button
                          type="button"
                          className={`message-save-button ${replySaved ? "saved" : ""}`}
                          title={replySaved ? "Unsave message" : "Save message"}
                          onPointerDown={(event) => event.stopPropagation()}
                          onClick={(event) => {
                            event.stopPropagation();
                            onToggleMessageSaved(reply, !replySaved);
                          }}
                        >
                          <Bookmark size={13} />
                          {replySaved ? "Saved" : "Save"}
                        </button>
                      </div>
                    )}
                    {reply.delivery_state !== "streaming" && <MessageMarkdown body={reply.body} />}
                    <MessageAttachments attachments={reply.attachments} />
                    <MessageArtifacts artifacts={reply.artifacts} onOpenArtifact={openArtifact} />
                    {reply.delivery_state === "error" && (
                      <div className="message-stream-state error">Response interrupted</div>
                    )}
                  </div>
                </article>
              );
            })}
          </section>
          {messageMenu && (
            <MessageActionMenu
              x={messageMenu.x}
              y={messageMenu.y}
              isSaved={savedMessageIds.has(messageMenu.message.id)}
              onCopyLink={() => copyMessageLink(messageMenu.message)}
              onCopyMarkdown={() => copyMessageMarkdown(messageMenu.message)}
              onToggleSaved={() => {
                onToggleMessageSaved(messageMenu.message, !savedMessageIds.has(messageMenu.message.id));
                setMessageMenu(null);
              }}
              onClose={() => setMessageMenu(null)}
            />
          )}
          {activeRoot && showBackToBottom && (
            <button type="button" className="thread-back-to-bottom" onClick={returnThreadToBottom}>
              <ArrowDown size={15} />
              Back to bottom
            </button>
          )}
        </div>

        <section
          className={`reply-composer ${isReplyDragOver ? "drag-over" : ""}`}
          onDragEnter={handleReplyDragEnter}
          onDragOver={handleReplyDragOver}
          onDragLeave={handleReplyDragLeave}
          onDrop={handleReplyDrop}
        >
          {mentionState && mentionCandidates.length > 0 && (
            <div className="mention-picker">
              {mentionCandidates.map((agent, index) => (
                <button
                  key={agent.id}
                  className={index === mentionIndex ? "active" : ""}
                  onMouseDown={(event) => {
                    event.preventDefault();
                    chooseMention(agent);
                  }}
                >
                  <AgentAvatar agent={agent} size="sm" title={`@${agent.handle}`} />
                  <span className="mention-picker-copy">
                    <strong>{agent.display_name}</strong>
                    <small>@{agent.handle}</small>
                    {visibleAgentDescription(agent.description) && <em>{visibleAgentDescription(agent.description)}</em>}
                  </span>
                </button>
              ))}
            </div>
          )}
          <input
            ref={fileInputRef}
            type="file"
            multiple
            className="file-input-hidden"
            onChange={(event) => {
              if (event.target.files) addReplyAttachments(event.target.files);
              event.target.value = "";
            }}
          />
          <DraftAttachmentsPreview attachments={replyAttachments} onRemove={removeReplyAttachment} />
          <textarea
            ref={textareaRef}
            value={replyDraft}
            onChange={(event) => {
              setReplyDraft(event.target.value);
              refreshMentionState(event.target.value, event.target.selectionStart);
            }}
            onSelect={(event) => refreshMentionState(replyDraft, event.currentTarget.selectionStart)}
            onKeyDown={handleReplyKeyDown}
            onPaste={handleReplyPaste}
            disabled={!activeRoot}
            placeholder={activeRoot ? isDm ? `Reply to @${dmAgent?.handle || "agent"}` : "Reply in thread" : "Select a thread to reply"}
          />
          <div className="reply-composer-actions">
            <button
              type="button"
              className="attach-button"
              disabled={!activeRoot}
              onClick={() => fileInputRef.current?.click()}
            >
              <Paperclip size={15} />
            </button>
            <button
              type="button"
              className="reply-send"
              title="Send reply"
              aria-label="Send reply"
              disabled={!activeRoot || (!replyDraft.trim() && replyAttachments.length === 0)}
              onClick={submitReply}
            >
              <Reply size={16} />
            </button>
          </div>
        </section>
      </section>

    </aside>
  );
}
