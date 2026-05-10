import { MessageSquare, Paperclip, Reply, X } from "lucide-react";
import { useEffect, useRef, useState, type ClipboardEvent, type DragEvent, type KeyboardEvent, type PointerEvent as ReactPointerEvent } from "react";
import { useMentionPicker } from "../hooks/useMentionPicker";
import { isImeComposing } from "../input-utils";
import { Agent, Artifact, Channel, DraftAttachment, Message, TASK_STATUSES, Task } from "../types";
import { formatTime } from "../ui-utils";
import { DraftAttachmentsPreview } from "./DraftAttachmentsPreview";
import { MessageAttachments } from "./MessageAttachments";
import { MessageArtifacts } from "./MessageArtifacts";
import { MessageMarkdown } from "./MessageMarkdown";

type ThreadPanelProps = {
  channel: Channel | null;
  agents: Agent[];
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
  openArtifact: (artifact: Artifact) => void;
  onResizeStart: (event: ReactPointerEvent<HTMLButtonElement>) => void;
};

function wasEdited(message: Message) {
  const created = new Date(message.created_at).getTime();
  const updated = new Date(message.updated_at).getTime();
  return Number.isFinite(created) && Number.isFinite(updated) && updated - created > 1000;
}

export function ThreadPanel({
  channel,
  agents,
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
  openArtifact,
  onResizeStart,
}: ThreadPanelProps) {
  const [isReplyDragOver, setIsReplyDragOver] = useState(false);
  const replyDragDepthRef = useRef(0);
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
  } = useMentionPicker({ agents, value: replyDraft, setValue: setReplyDraft, textareaRef });

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
  }, [activeRoot?.id]);

  return (
    <aside className="thread">
      <button
        className="thread-resize-handle"
        aria-label="Resize thread panel"
        onPointerDown={onResizeStart}
      />
      <header>
        <div>
          <h2>
            Thread <span>{channel ? isDm ? `- @${dmAgent?.handle || "agent"}` : `- #${channel.name}` : "- no channel"}</span>
          </h2>
          <p>
            {activeRoot ? `${replies.length} ${replies.length === 1 ? "reply" : "replies"}` : "No thread selected"}
            {unreadCount > 0 ? ` · ${unreadCount} new` : ""}
          </p>
        </div>
        <button type="button" onClick={onClose} aria-label="Close thread panel"><X size={18} /></button>
      </header>

      <section className="thread-focus">
        <div className="thread-scroll">
          {activeRoot && (
            <article className={`thread-root ${activeRoot.sender_role === "system" ? "system-message" : ""}`}>
              {activeRoot.sender_role === "system" ? (
                <div className="system-message-line">
                  <MessageMarkdown body={activeRoot.body} />
                  <time>{formatTime(activeRoot.created_at)}</time>
                </div>
              ) : (
                <>
                  <div className="meta">
                    <strong>{activeRoot.sender_name}</strong>
                    <time>{formatTime(activeRoot.created_at)}</time>
                    {wasEdited(activeRoot) && <span className="edited-indicator">edited</span>}
                  </div>
                  <MessageMarkdown body={activeRoot.body} />
                  <MessageAttachments attachments={activeRoot.attachments} />
                  <MessageArtifacts artifacts={activeRoot.artifacts} onOpenArtifact={openArtifact} />
                  {activeRoot.delivery_state === "streaming" && (
                    <div className="message-stream-state">Streaming response...</div>
                  )}
                  {activeRoot.delivery_state === "error" && (
                    <div className="message-stream-state error">Response interrupted</div>
                  )}
                </>
              )}
            </article>
          )}

          {activeTask && (
            <section className="thread-task-card">
              <div className="task-card-head">
                <span>Task #{activeTask.number}</span>
                <strong>{activeTask.status.replace("_", " ")}</strong>
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
              <select
                value={activeTask.assignee_id ?? ""}
                onChange={(event) => claimTask(activeTask, event.target.value)}
              >
                <option value="">Unassigned</option>
                {agents.map((agent) => (
                  <option key={agent.id} value={agent.id}>{agent.display_name}</option>
                ))}
              </select>
              <div className="status-row">
                {TASK_STATUSES.map((status) => (
                  <button
                    key={status}
                    className={activeTask.status === status ? "active" : ""}
                    onClick={() => updateTaskStatus(activeTask, status)}
                  >
                    {status.replace("_", " ")}
                  </button>
                ))}
              </div>
            </section>
          )}

          <section className="reply-list">
            {!activeRoot && (
              <div className="empty-state compact">
                <MessageSquare size={28} />
                <h2>No thread selected</h2>
                <p>Select a root message after you create one.</p>
              </div>
            )}
            {replies.map((reply) => {
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
                <article key={reply.id}>
                  <div className="avatar tiny">{reply.sender_name.slice(0, 1)}</div>
                  <div>
                    <div className="meta">
                      <strong>{reply.sender_name}</strong>
                      <time>{formatTime(reply.created_at)}</time>
                      {wasEdited(reply) && <span className="edited-indicator">edited</span>}
                    </div>
                    <MessageMarkdown body={reply.body} />
                    <MessageAttachments attachments={reply.attachments} />
                    <MessageArtifacts artifacts={reply.artifacts} onOpenArtifact={openArtifact} />
                    {reply.delivery_state === "streaming" && (
                      <div className="message-stream-state">Streaming response...</div>
                    )}
                    {reply.delivery_state === "error" && (
                      <div className="message-stream-state error">Response interrupted</div>
                    )}
                  </div>
                </article>
              );
            })}
          </section>
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
              if (event.target.files) addReplyAttachments(event.target.files);
              event.target.value = "";
            }}
          />
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
          <DraftAttachmentsPreview attachments={replyAttachments} onRemove={removeReplyAttachment} />
          <div className="reply-composer-actions">
            <div className="reply-composer-buttons">
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
                disabled={!activeRoot || (!replyDraft.trim() && replyAttachments.length === 0)}
                onClick={submitReply}
              >
                Reply <Reply size={15} />
              </button>
            </div>
          </div>
        </section>
      </section>

    </aside>
  );
}
