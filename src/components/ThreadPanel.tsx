import { MessageSquare, Reply, X } from "lucide-react";
import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  filterMentionAgents,
  getMentionState,
  insertAgentMention,
  mentionedAgentsForBody,
  type MentionState,
} from "../mentions";
import { Agent, Bootstrap, Channel, Message, TASK_STATUSES, Task } from "../types";
import { formatTime } from "../ui-utils";

type ThreadPanelProps = {
  data: Bootstrap;
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
  data,
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
  const rootWorkItems = useMemo(() => {
    if (!activeRoot) return [];
    return data.agent_work_items.filter(
      (item) =>
        item.source_message_id === activeRoot.id ||
        (!item.source_message_id && item.thread_root_id === activeRoot.id),
    );
  }, [data.agent_work_items, activeRoot]);

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

  function openMentionPicker() {
    const textarea = textareaRef.current;
    const cursor = textarea?.selectionStart ?? replyDraft.length;
    const prefix = replyDraft.slice(0, cursor);
    const suffix = replyDraft.slice(cursor);
    const separator = prefix.length > 0 && !/\s$/.test(prefix) ? " " : "";
    const nextText = `${prefix}${separator}@${suffix}`;
    const nextCursor = prefix.length + separator.length + 1;
    setReplyDraft(nextText);
    setMentionState({ query: "", start: nextCursor - 1, end: nextCursor });
    setMentionIndex(0);
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
      if (activeRoot && replyDraft.trim()) {
        sendReply();
        setMentionState(null);
      }
    }
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
            {rootWorkItems.length > 0 && (
              <div className="agent-mention-line">
                {rootWorkItems.map((item) => (
                  <strong key={item.id}>@{item.agent_handle} {item.status}</strong>
                ))}
              </div>
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
                if (event.key === "Enter") saveTaskTitle(activeTask);
              }}
            />
            <select
              value={activeTask.assignee_id ?? ""}
              onChange={(event) => claimTask(activeTask, event.target.value)}
            >
              <option value="">Unassigned</option>
              {data.agents.map((agent) => (
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
            const mentionedAgents = mentionedAgentsForBody(reply.body, agents);
            const replyWorkItems = data.agent_work_items.filter((item) => item.source_message_id === reply.id);
            return (
              <article key={reply.id}>
                <div className="avatar tiny">{reply.sender_name.slice(0, 1)}</div>
                <div>
                  <div className="meta">
                    <strong>{reply.sender_name}</strong>
                    <time>{formatTime(reply.created_at)}</time>
                  </div>
                  <p>{reply.body}</p>
                  {(mentionedAgents.length > 0 || replyWorkItems.length > 0) && (
                    <div className="agent-mention-line">
                      {mentionedAgents.map((agent) => (
                        <span key={agent.id}>@{agent.handle}</span>
                      ))}
                      {replyWorkItems.map((item) => (
                        <strong key={item.id}>@{item.agent_handle} {item.status}</strong>
                      ))}
                    </div>
                  )}
                </div>
              </article>
            );
          })}
        </section>

        <section className="reply-composer">
          <div className="composer-label">
            <button type="button" disabled={!activeRoot || agents.length === 0} onClick={openMentionPicker}>Add Agent</button>
            <span>{agents.length === 0 ? "Add an agent before assigning work." : "Mention an agent to assign work in this thread."}</span>
          </div>
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
            placeholder={activeRoot ? "Reply in thread; type @ or Add Agent to assign work" : "Select a thread to reply"}
          />
          <button disabled={!activeRoot || !replyDraft.trim()} onClick={sendReply}>
            Reply <Reply size={15} />
          </button>
        </section>
      </section>

    </aside>
  );
}
