import { MessageSquare, Pencil, Reply, Trash2, X } from "lucide-react";
import { useRef, type KeyboardEvent } from "react";
import { useMentionPicker } from "../hooks/useMentionPicker";
import { Agent, Channel, Message, TASK_STATUSES, Task } from "../types";
import { formatTime } from "../ui-utils";
import { MessageMarkdown } from "./MessageMarkdown";

type ThreadPanelProps = {
  channel: Channel | null;
  agents: Agent[];
  activeRoot: Message | null;
  activeTask: Task | null;
  replies: Message[];
  taskTitleDrafts: Record<string, string>;
  replyDraft: string;
  toggleThreadFollow: (message: Message) => void;
  setActiveThreadId: (threadId: string | null) => void;
  setTaskTitleDraft: (task: Task, title: string) => void;
  saveTaskTitle: (task: Task) => void;
  claimTask: (task: Task, agentId: string) => void;
  updateTaskStatus: (task: Task, status: string) => void;
  editMessage: (message: Message) => void;
  deleteMessage: (message: Message) => void;
  setReplyDraft: (value: string) => void;
  sendReply: () => void;
};

export function ThreadPanel({
  channel,
  agents,
  activeRoot,
  activeTask,
  replies,
  taskTitleDrafts,
  replyDraft,
  toggleThreadFollow,
  setActiveThreadId,
  setTaskTitleDraft,
  saveTaskTitle,
  claimTask,
  updateTaskStatus,
  editMessage,
  deleteMessage,
  setReplyDraft,
  sendReply,
}: ThreadPanelProps) {
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
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
    if (handleMentionKeyDown(event)) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submitReply();
    }
  }

  function submitReply() {
    if (!activeRoot || !replyDraft.trim()) return;
    sendReply();
    closeMentionPicker();
    focusComposer();
  }

  return (
    <aside className="thread">
      <header>
        <div>
          <h2>
            Thread <span>{channel ? isDm ? `- @${dmAgent?.handle || "agent"}` : `- #${channel.name}` : "- no channel"}</span>
          </h2>
          <p>{activeRoot ? `Root ${activeRoot.id.slice(0, 8)}` : "No thread selected"}</p>
        </div>
        {activeRoot && (
          <button onClick={() => toggleThreadFollow(activeRoot)}>
            {activeRoot.thread_followed ? "Following" : "Muted"}
          </button>
        )}
        <button onClick={() => setActiveThreadId(null)}><X size={18} /></button>
      </header>

      <section className="thread-focus">
        {activeRoot && (
          <article className="thread-root">
            <div className="meta">
              <strong>{activeRoot.sender_name}</strong>
              <time>{formatTime(activeRoot.created_at)}</time>
            </div>
            <MessageMarkdown body={activeRoot.body} />
            <div className="message-actions visible">
              <button className="reply-pill neutral" onClick={() => editMessage(activeRoot)}>
                <Pencil size={14} /> Edit
              </button>
              <button className="reply-pill danger" onClick={() => deleteMessage(activeRoot)}>
                <Trash2 size={14} /> Delete
              </button>
            </div>
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
            return (
              <article key={reply.id}>
                <div className="avatar tiny">{reply.sender_name.slice(0, 1)}</div>
                <div>
                  <div className="meta">
                    <strong>{reply.sender_name}</strong>
                    <time>{formatTime(reply.created_at)}</time>
                  </div>
                  <MessageMarkdown body={reply.body} />
                  <div className="message-actions visible">
                    <button className="reply-pill neutral" onClick={() => editMessage(reply)}>
                      <Pencil size={14} /> Edit
                    </button>
                    <button className="reply-pill danger" onClick={() => deleteMessage(reply)}>
                      <Trash2 size={14} /> Delete
                    </button>
                  </div>
                </div>
              </article>
            );
          })}
        </section>

        <section className="reply-composer">
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
                  <small>{agent.display_name} · {agent.runtime} · {agent.status}</small>
                </button>
              ))}
            </div>
          )}
          <textarea
            ref={textareaRef}
            value={replyDraft}
            onChange={(event) => {
              setReplyDraft(event.target.value);
              refreshMentionState(event.target.value, event.target.selectionStart);
            }}
            onSelect={(event) => refreshMentionState(replyDraft, event.currentTarget.selectionStart)}
            onKeyDown={handleReplyKeyDown}
            disabled={!activeRoot}
            placeholder={activeRoot ? isDm ? `Reply to @${dmAgent?.handle || "agent"}` : "Reply in thread" : "Select a thread to reply"}
          />
          <button className="reply-send" disabled={!activeRoot || !replyDraft.trim()} onClick={submitReply}>
            Reply <Reply size={15} />
          </button>
        </section>
      </section>

    </aside>
  );
}
