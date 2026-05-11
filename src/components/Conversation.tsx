import {
  CheckCircle2,
  Hash,
  LayoutList,
  Menu,
  MessageSquare,
  Paperclip,
  Plus,
  Send,
  Settings,
  Trash2,
} from "lucide-react";
import { useEffect, useLayoutEffect, useRef, useState, type ClipboardEvent, type DragEvent, type FocusEvent, type KeyboardEvent } from "react";
import { useMentionPicker } from "../hooks/useMentionPicker";
import { isImeComposing } from "../input-utils";
import { Agent, Artifact, Channel, DraftAttachment, Message, TASK_STATUSES, Task } from "../types";
import { firstLines, formatTime } from "../ui-utils";
import { AgentAvatar } from "./AgentAvatar";
import { DraftAttachmentsPreview } from "./DraftAttachmentsPreview";
import { MessageAttachments } from "./MessageAttachments";
import { MessageArtifacts } from "./MessageArtifacts";
import { MessageMarkdown } from "./MessageMarkdown";

type ConversationProps = {
  channel: Channel | null;
  agents: Agent[];
  channelAgents: Agent[];
  activeTab: "chat" | "tasks";
  activeRoot: Message | null;
  rootMessages: Message[];
  threadReplyCounts: Record<string, number>;
  visibleTasks: Task[];
  draft: string;
  draftAttachments: DraftAttachment[];
  taskDraft: string;
  taskTitleDrafts: Record<string, string>;
  setActiveTab: (tab: "chat" | "tasks") => void;
  setActiveThreadId: (threadId: string | null) => void;
  openMobileSidebar: () => void;
  openChannelSettingsModal: () => void;
  deleteChannel: () => void;
  openChannelAgentsModal: () => void;
  taskForMessage: (messageId: string) => Task | null;
  setTaskTitleDraft: (task: Task, title: string) => void;
  saveTaskTitle: (task: Task) => void;
  claimTask: (task: Task, agentId: string) => void;
  updateTaskStatus: (task: Task, status: string) => void;
  openTask: (task: Task) => void;
  setTaskDraft: (value: string) => void;
  createTaskFromBoard: () => void;
  setDraft: (value: string) => void;
  addDraftAttachments: (files: FileList | File[]) => void;
  removeDraftAttachment: (id: string) => void;
  sendRootMessage: (asTask?: boolean) => void;
  openAgentDetail: (agent: Agent) => void;
  openArtifact: (artifact: Artifact) => void;
};

function wasEdited(message: Message) {
  const created = new Date(message.created_at).getTime();
  const updated = new Date(message.updated_at).getTime();
  return Number.isFinite(created) && Number.isFinite(updated) && updated - created > 1000;
}

function agentForMessage(message: Message, agents: Agent[]) {
  if (message.sender_role !== "agent") return null;
  const sender = message.sender_name.replace(/^@/, "");
  return agents.find((agent) => agent.handle === sender || agent.display_name === message.sender_name) ?? null;
}

export function Conversation({
  channel,
  agents,
  channelAgents,
  activeTab,
  activeRoot,
  rootMessages,
  threadReplyCounts,
  visibleTasks,
  draft,
  draftAttachments,
  taskDraft,
  taskTitleDrafts,
  setActiveTab,
  setActiveThreadId,
  openMobileSidebar,
  openChannelSettingsModal,
  deleteChannel,
  openChannelAgentsModal,
  taskForMessage,
  setTaskTitleDraft,
  saveTaskTitle,
  claimTask,
  updateTaskStatus,
  openTask,
  setTaskDraft,
  createTaskFromBoard,
  setDraft,
  addDraftAttachments,
  removeDraftAttachment,
  sendRootMessage,
  openAgentDetail,
  openArtifact,
}: ConversationProps) {
  const [sendAsTask, setSendAsTask] = useState(false);
  const [isComposerDragOver, setIsComposerDragOver] = useState(false);
  const [showChannelActions, setShowChannelActions] = useState(false);
  const composerDragDepthRef = useRef(0);
  const messageListRef = useRef<HTMLDivElement | null>(null);
  const shouldFollowMessagesRef = useRef(true);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const isDm = channel?.kind === "dm";
  const dmAgent = isDm ? agents.find((agent) => agent.id === channel?.dm_agent_id) ?? null : null;
  const {
    mentionState,
    mentionIndex,
    mentionCandidates,
    refreshMentionState,
    chooseMention,
    handleMentionKeyDown,
    closeMentionPicker,
    focusComposer,
  } = useMentionPicker({ agents, value: draft, setValue: setDraft, textareaRef });
  const lastRootMessage = rootMessages[rootMessages.length - 1] ?? null;

  function isMessageListAtBottom(element: HTMLDivElement) {
    return element.scrollHeight - element.scrollTop - element.clientHeight < 32;
  }

  function scrollMessagesToBottom(behavior: ScrollBehavior = "auto") {
    const element = messageListRef.current;
    if (!element) return;
    element.scrollTo({ top: element.scrollHeight, behavior });
  }

  function handleMessageListScroll() {
    const element = messageListRef.current;
    if (!element) return;
    shouldFollowMessagesRef.current = isMessageListAtBottom(element);
  }

  function handleMessageListContentLoad() {
    if (!shouldFollowMessagesRef.current) return;
    scrollMessagesToBottom();
  }

  function handleComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (isImeComposing(event)) return;
    if (handleMentionKeyDown(event)) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submitComposer();
    }
  }

  function submitComposer() {
    if (!channel || (!draft.trim() && draftAttachments.length === 0)) return;
    sendRootMessage(isDm ? false : sendAsTask);
    closeMentionPicker();
    focusComposer();
  }

  function hasDraggedFiles(event: DragEvent<HTMLElement>) {
    return Array.from(event.dataTransfer.types).includes("Files");
  }

  function handleComposerDragEnter(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    composerDragDepthRef.current += 1;
    event.dataTransfer.dropEffect = channel ? "copy" : "none";
    if (channel) setIsComposerDragOver(true);
  }

  function handleComposerDragOver(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = channel ? "copy" : "none";
    if (channel) setIsComposerDragOver(true);
  }

  function handleComposerDragLeave(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    composerDragDepthRef.current = Math.max(0, composerDragDepthRef.current - 1);
    if (composerDragDepthRef.current === 0) setIsComposerDragOver(false);
  }

  function handleComposerDrop(event: DragEvent<HTMLElement>) {
    if (!hasDraggedFiles(event)) return;
    event.preventDefault();
    event.stopPropagation();
    composerDragDepthRef.current = 0;
    setIsComposerDragOver(false);
    if (!channel || event.dataTransfer.files.length === 0) return;
    addDraftAttachments(event.dataTransfer.files);
    focusComposer();
  }

  function handleComposerPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const imageFiles = Array.from(event.clipboardData.files).filter((file) => file.type.startsWith("image/"));
    if (imageFiles.length === 0) return;
    event.preventDefault();
    if (!channel) return;
    addDraftAttachments(imageFiles);
    focusComposer();
  }

  useEffect(() => {
    if (!isDm) return;
    setSendAsTask(false);
    if (activeTab === "tasks") setActiveTab("chat");
  }, [activeTab, isDm, setActiveTab]);

  useEffect(() => {
    composerDragDepthRef.current = 0;
    setIsComposerDragOver(false);
    setShowChannelActions(false);
  }, [channel?.id]);

  function handleChannelActionsBlur(event: FocusEvent<HTMLDivElement>) {
    if (event.currentTarget.contains(event.relatedTarget)) return;
    setShowChannelActions(false);
  }

  useLayoutEffect(() => {
    shouldFollowMessagesRef.current = true;
    scrollMessagesToBottom();
  }, [channel?.id]);

  useLayoutEffect(() => {
    if (!shouldFollowMessagesRef.current) return;
    scrollMessagesToBottom();
  }, [activeTab, channel?.id, rootMessages.length, lastRootMessage?.id, lastRootMessage?.updated_at, lastRootMessage?.delivery_state]);

  return (
    <section className="conversation">
      <header className="topbar">
        <button
          type="button"
          className="mobile-nav-button"
          aria-label="Open navigation"
          onClick={openMobileSidebar}
        >
          <Menu size={18} />
        </button>
        <div className="channel-title">
          {isDm && dmAgent ? (
            <button
              type="button"
              className="hash-card dm-card dm-agent-detail-trigger"
              title={`View @${dmAgent.handle} details`}
              aria-label={`View @${dmAgent.handle} details`}
              onClick={() => openAgentDetail(dmAgent)}
            >
              <AgentAvatar agent={dmAgent} />
            </button>
          ) : (
            <span className="hash-card">
              <Hash />
            </span>
          )}
          <div>
            <h1>{isDm ? dmAgent?.display_name || "Direct Message" : channel?.name || "No channel"}</h1>
            <p>
              {isDm
                ? dmAgent ? `@${dmAgent.handle} · ${dmAgent.runtime} · ${dmAgent.status}` : "Agent no longer exists"
                : channel?.description || "Create a channel from the sidebar"}
            </p>
            {channel && !isDm && (
              <div className="channel-agent-strip">
                <span>Agents</span>
                {channelAgents.length > 0 ? (
                  channelAgents.slice(0, 5).map((agent) => (
                    <button key={agent.id} type="button" onClick={openChannelAgentsModal}>
                      <span className={`mini-dot ${agent.status}`} />
                      @{agent.handle}
                    </button>
                  ))
                ) : (
                  <button type="button" className="empty" onClick={openChannelAgentsModal}>No agents</button>
                )}
                <button type="button" className="add-channel-agent" onClick={openChannelAgentsModal}>
                  <Plus size={13} />
                </button>
              </div>
            )}
          </div>
        </div>
        {channel && !isDm && (
          <div className="channel-header-actions" onBlur={handleChannelActionsBlur}>
            <button
              type="button"
              className={`channel-action-trigger ${showChannelActions ? "active" : ""}`}
              title="Channel actions"
              aria-label="Channel actions"
              aria-expanded={showChannelActions}
              onClick={() => setShowChannelActions((current) => !current)}
            >
              <Settings size={18} />
            </button>
            {showChannelActions && (
              <div className="channel-actions-menu">
                <button
                  type="button"
                  onClick={() => {
                    setShowChannelActions(false);
                    openChannelSettingsModal();
                  }}
                >
                  <Settings size={15} />
                  <span>Channel settings</span>
                </button>
                <button
                  type="button"
                  className="danger"
                  onClick={() => {
                    setShowChannelActions(false);
                    deleteChannel();
                  }}
                >
                  <Trash2 size={15} />
                  <span>Delete channel</span>
                </button>
              </div>
            )}
          </div>
        )}
      </header>

      <div className="tabs">
        <button className={activeTab === "chat" ? "active" : ""} onClick={() => setActiveTab("chat")}>
          <MessageSquare size={16} /> Chat
        </button>
        {!isDm && (
          <button className={activeTab === "tasks" ? "active" : ""} onClick={() => setActiveTab("tasks")}>
            <LayoutList size={16} /> Tasks
          </button>
        )}
      </div>

      {activeTab === "chat" ? (
        <div
          ref={messageListRef}
          className="message-list"
          onScroll={handleMessageListScroll}
          onLoadCapture={handleMessageListContentLoad}
        >
          {channel ? (
            rootMessages.length > 0 ? (
              <div className="beginning">
                {isDm ? `Beginning of your DM with @${dmAgent?.handle || "agent"}` : `Beginning of #${channel.name}`}
              </div>
            ) : (
              <div className="empty-state">
                <MessageSquare size={34} />
                <h2>{isDm ? "No DM messages yet" : "No messages yet"}</h2>
                <p>
                  {isDm
                    ? "Send a message here to talk directly with this agent."
                    : "Send a root message from the composer. Replies belong in the right thread pane."}
                </p>
              </div>
            )
          ) : (
            <div className="empty-state">
              <Hash size={34} />
              <h2>No channels yet</h2>
              <p>Create a channel in the left sidebar, then send messages or tasks.</p>
            </div>
          )}
          {rootMessages.map((message) => {
            const linkedTask = taskForMessage(message.id);
            const replyCount = threadReplyCounts[message.id] ?? 0;
            const messageAgent = isDm ? null : agentForMessage(message, agents);
            if (message.sender_role === "system") {
              return (
                <article key={message.id} className="system-message">
                  <div className="system-message-line">
                    <MessageMarkdown body={message.body} />
                    <time>{formatTime(message.created_at)}</time>
                  </div>
                </article>
              );
            }
            return (
              <article
                key={message.id}
                className={`message-card ${message.id === activeRoot?.id ? "focused" : ""}`}
                onClick={() => setActiveThreadId(message.id)}
              >
                {messageAgent ? (
                  <button
                    type="button"
                    className="message-agent-avatar-trigger"
                    title={`View @${messageAgent.handle} details`}
                    onClick={(event) => {
                      event.stopPropagation();
                      openAgentDetail(messageAgent);
                    }}
                  >
                    <AgentAvatar agent={messageAgent} />
                  </button>
                ) : (
                  <div className="avatar">{message.sender_name.slice(0, 1)}</div>
                )}
                <div className="message-body">
                  <div className="meta">
                    <strong>{message.sender_name}</strong>
                    <span>{message.sender_role}</span>
                    <time>{formatTime(message.created_at)}</time>
                    {wasEdited(message) && <span className="edited-indicator">edited</span>}
                    {linkedTask && (
                      <mark>
                        <CheckCircle2 size={14} /> #{linkedTask.number} · {linkedTask.status.replace("_", " ")}
                      </mark>
                    )}
                  </div>
                  <MessageMarkdown body={firstLines(message.body)} />
                  <MessageAttachments attachments={message.attachments} />
                  <MessageArtifacts artifacts={message.artifacts} onOpenArtifact={openArtifact} />
                  {message.delivery_state === "streaming" && (
                    <div className="message-stream-state">Streaming response...</div>
                  )}
                  {message.delivery_state === "error" && (
                    <div className="message-stream-state error">Response interrupted</div>
                  )}
                  {replyCount > 0 && <div className="thread-reply-count">{replyCount} {replyCount === 1 ? "reply" : "replies"}</div>}
                </div>
              </article>
            );
          })}
        </div>
      ) : (
        <div className="task-board">
          <section className="task-create">
            <div>
              <h2>Create task in {channel ? `#${channel.name}` : "a channel"}</h2>
              <p>Tasks are top-level messages with status, assignee, and a thread.</p>
            </div>
            <textarea
              value={taskDraft}
              onChange={(event) => setTaskDraft(event.target.value)}
              disabled={!channel}
              placeholder={channel ? "Task title or short brief" : "Create a channel before creating tasks"}
            />
            <button disabled={!channel || !taskDraft.trim()} onClick={createTaskFromBoard}>
              <Plus size={15} /> Create Task
            </button>
          </section>
          {visibleTasks.length === 0 && (
            <div className="empty-state">
              <LayoutList size={34} />
              <h2>No tasks in this channel</h2>
              <p>Create a task above or switch the composer to Task mode for explicit tracked work.</p>
            </div>
          )}
          {visibleTasks.map((task) => (
            <article className="task-card" key={task.id}>
              <div className="task-card-head">
                <span>#{task.number}</span>
                <button onClick={() => openTask(task)}>
                  <MessageSquare size={14} /> Open thread
                </button>
              </div>
              <input
                value={taskTitleDrafts[task.id] ?? task.title}
                onChange={(event) => setTaskTitleDraft(task, event.target.value)}
                onBlur={() => saveTaskTitle(task)}
                onKeyDown={(event) => {
                  if (isImeComposing(event)) return;
                  if (event.key === "Enter") saveTaskTitle(task);
                }}
              />
              <p>{task.channel_name} · {task.assignee_name || "unassigned"} · updated {formatTime(task.updated_at)}</p>
              <div className="task-controls">
                <select value={task.assignee_id ?? ""} onChange={(event) => claimTask(task, event.target.value)}>
                  <option value="">Unassigned</option>
                  {agents.map((agent) => (
                    <option key={agent.id} value={agent.id}>{agent.display_name}</option>
                  ))}
                </select>
                <div className="status-row">
                  {TASK_STATUSES.map((status) => (
                    <button
                      key={status}
                      className={task.status === status ? "active" : ""}
                      onClick={() => updateTaskStatus(task, status)}
                    >
                      {status.replace("_", " ")}
                    </button>
                  ))}
                </div>
              </div>
            </article>
          ))}
        </div>
      )}

      <footer
        className={`composer ${isComposerDragOver ? "drag-over" : ""}`}
        onDragEnter={handleComposerDragEnter}
        onDragOver={handleComposerDragOver}
        onDragLeave={handleComposerDragLeave}
        onDrop={handleComposerDrop}
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
                <span>@{agent.handle}</span>
                <small>{agent.display_name} · {agent.role || "agent"} · {agent.runtime} · {agent.status}</small>
                {agent.description && <em>{agent.description}</em>}
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
            if (event.target.files) addDraftAttachments(event.target.files);
            event.target.value = "";
          }}
        />
        <textarea
          ref={textareaRef}
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            refreshMentionState(event.target.value, event.target.selectionStart);
          }}
          onSelect={(event) => refreshMentionState(draft, event.currentTarget.selectionStart)}
          onKeyDown={handleComposerKeyDown}
          onPaste={handleComposerPaste}
          disabled={!channel}
          placeholder={
            channel
              ? isDm
                ? `Message @${dmAgent?.handle || "agent"}`
                : `Message #${channel.name} - type @ to send to an agent`
              : "Create a channel before messaging"
          }
        />
        <DraftAttachmentsPreview attachments={draftAttachments} onRemove={removeDraftAttachment} />
        <div className="composer-actions">
          {!isDm && (
            <div className="send-mode" aria-label="Send mode">
              <button className={!sendAsTask ? "active" : ""} onClick={() => setSendAsTask(false)}>Message</button>
              <button className={sendAsTask ? "active" : ""} onClick={() => setSendAsTask(true)}>Task</button>
            </div>
          )}
          <button
            type="button"
            className="attach-button"
            disabled={!channel}
            onClick={() => fileInputRef.current?.click()}
          >
            <Paperclip size={16} />
          </button>
          <button className="send" disabled={!channel || (!draft.trim() && draftAttachments.length === 0)} onClick={submitComposer}>
            Send <Send size={15} />
          </button>
        </div>
      </footer>
    </section>
  );
}
