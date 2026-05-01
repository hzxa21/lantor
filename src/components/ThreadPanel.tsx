import { MessageSquare, Reply, X } from "lucide-react";
import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  filterMentionAgents,
  getMentionState,
  insertAgentMention,
  type MentionState,
} from "../mentions";
import { Agent, Channel, Message, TASK_STATUSES, Task } from "../types";
import { formatTime } from "../ui-utils";

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
  setReplyDraft,
  sendReply,
}: ThreadPanelProps) {
  const [mentionState, setMentionState] = useState<MentionState | null>(null);
  const [mentionIndex, setMentionIndex] = useState(0);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const mentionCandidates = useMemo(() => {
    return mentionState ? filterMentionAgents(agents, mentionState.query) : [];
  }, [agents, mentionState]);

  function refreshMentionState(text: string, cursor: number) {
    setMentionState(getMentionState(text, cursor));
    setMentionIndex(0);
  }

  function chooseMention(agent: Agent) {
    if (!mentionState) return;
    const { nextText, nextCursor } = insertAgentMention(replyDraft, mentionState, agent.handle);
    setReplyDraft(nextText);
    setMentionState(null);
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
    });
  }

  function handleReplyKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (mentionState && mentionCandidates.length > 0) {
      if (event.key === "ArrowDown") {
        event.preventDefault();
        setMentionIndex((current) => (current + 1) % mentionCandidates.length);
        return;
      }
      if (event.key === "ArrowUp") {
        event.preventDefault();
        setMentionIndex((current) => (current - 1 + mentionCandidates.length) % mentionCandidates.length);
        return;
      }
      if (event.key === "Enter" || event.key === "Tab") {
        event.preventDefault();
        chooseMention(mentionCandidates[mentionIndex] ?? mentionCandidates[0]);
        return;
      }
      if (event.key === "Escape") {
        event.preventDefault();
        setMentionState(null);
        return;
      }
    }
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submitReply();
    }
  }

  function submitReply() {
    if (!activeRoot || !replyDraft.trim()) return;
    sendReply();
    setMentionState(null);
    window.requestAnimationFrame(() => textareaRef.current?.focus());
  }

  return (
    <aside className="thread">
      <header>
        <div>
          <h2>Thread <span>{channel ? `- #${channel.name}` : "- no channel"}</span></h2>
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
            <p>{activeRoot.body}</p>
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
                  <p>{reply.body}</p>
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
            placeholder={activeRoot ? "Reply in thread" : "Select a thread to reply"}
          />
          <button className="reply-send" disabled={!activeRoot || !replyDraft.trim()} onClick={submitReply}>
            Reply <Reply size={15} />
          </button>
        </section>
      </section>

    </aside>
  );
}
