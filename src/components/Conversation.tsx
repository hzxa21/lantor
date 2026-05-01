import {
  CheckCircle2,
  Hash,
  LayoutList,
  MessageSquare,
  PanelRight,
  Plus,
  Send,
} from "lucide-react";
import { useMemo, useRef, useState, type KeyboardEvent } from "react";
import {
  filterMentionAgents,
  getMentionState,
  insertAgentMention,
  mentionedAgentsForBody,
  type MentionState,
} from "../mentions";
import { Agent, AgentWorkItem, Channel, Message, TASK_STATUSES, Task } from "../types";
import { firstLines, formatTime } from "../ui-utils";

type ConversationProps = {
  channel: Channel | null;
  agents: Agent[];
  activeTab: "chat" | "tasks";
  activeRoot: Message | null;
  rootMessages: Message[];
  visibleTasks: Task[];
  workItems: AgentWorkItem[];
  draft: string;
  taskDraft: string;
  taskTitleDrafts: Record<string, string>;
  showThread: boolean;
  setActiveTab: (tab: "chat" | "tasks") => void;
  setActiveThreadId: (threadId: string | null) => void;
  setShowThread: (value: boolean) => void;
  taskForMessage: (messageId: string) => Task | null;
  toggleThreadFollow: (message: Message) => void;
  setTaskTitleDraft: (task: Task, title: string) => void;
  saveTaskTitle: (task: Task) => void;
  claimTask: (task: Task, agentId: string) => void;
  updateTaskStatus: (task: Task, status: string) => void;
  openTask: (task: Task) => void;
  setTaskDraft: (value: string) => void;
  createTaskFromBoard: () => void;
  setDraft: (value: string) => void;
  sendRootMessage: (asTask?: boolean) => void;
};

export function Conversation({
  channel,
  agents,
  activeTab,
  activeRoot,
  rootMessages,
  visibleTasks,
  workItems,
  draft,
  taskDraft,
  taskTitleDrafts,
  showThread,
  setActiveTab,
  setActiveThreadId,
  setShowThread,
  taskForMessage,
  toggleThreadFollow,
  setTaskTitleDraft,
  saveTaskTitle,
  claimTask,
  updateTaskStatus,
  openTask,
  setTaskDraft,
  createTaskFromBoard,
  setDraft,
  sendRootMessage,
}: ConversationProps) {
  const [sendAsTask, setSendAsTask] = useState(false);
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
    const { nextText, nextCursor } = insertAgentMention(draft, mentionState, agent.handle);
    setDraft(nextText);
    setMentionState(null);
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
    });
  }

  function openMentionPicker() {
    const textarea = textareaRef.current;
    const cursor = textarea?.selectionStart ?? draft.length;
    const prefix = draft.slice(0, cursor);
    const suffix = draft.slice(cursor);
    const separator = prefix.length > 0 && !/\s$/.test(prefix) ? " " : "";
    const nextText = `${prefix}${separator}@${suffix}`;
    const nextCursor = prefix.length + separator.length + 1;
    setDraft(nextText);
    setMentionState({ query: "", start: nextCursor - 1, end: nextCursor });
    setMentionIndex(0);
    window.requestAnimationFrame(() => {
      textareaRef.current?.focus();
      textareaRef.current?.setSelectionRange(nextCursor, nextCursor);
    });
  }

  function handleComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
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
      if (channel && draft.trim()) {
        sendRootMessage(sendAsTask);
        setMentionState(null);
      }
    }
  }
  return (
    <section className="conversation">
      <header className="topbar">
        <div className="channel-title">
          <span className="hash-card"><Hash /></span>
          <div>
            <h1>{channel?.name || "No channel"}</h1>
            <p>{channel?.description || "Create a channel from the sidebar"}</p>
          </div>
        </div>
        <div className="top-actions">
          <button
            className={`thread-toggle ${showThread ? "active" : ""}`}
            onClick={() => setShowThread(!showThread)}
            title={showThread ? "Hide thread panel" : "Show thread panel"}
          >
            <PanelRight size={16} />
          </button>
        </div>
      </header>

      <div className="tabs">
        <button className={activeTab === "chat" ? "active" : ""} onClick={() => setActiveTab("chat")}>
          <MessageSquare size={16} /> Chat
        </button>
        <button className={activeTab === "tasks" ? "active" : ""} onClick={() => setActiveTab("tasks")}>
          <LayoutList size={16} /> Tasks
        </button>
      </div>

      {activeTab === "chat" ? (
        <div className="message-list">
          {channel ? (
            rootMessages.length > 0 ? (
              <div className="beginning">Beginning of #{channel.name}</div>
            ) : (
              <div className="empty-state">
                <MessageSquare size={34} />
                <h2>No messages yet</h2>
                <p>Send a root message from the composer. Replies belong in the right thread pane.</p>
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
            const messageWorkItems = workItems.filter(
              (item) =>
                item.source_message_id === message.id ||
                (!item.source_message_id && item.thread_root_id === message.id),
            );
            const mentionedAgents = mentionedAgentsForBody(message.body, agents);
            return (
              <article
                key={message.id}
                className={`message-card ${message.id === activeRoot?.id ? "focused" : ""}`}
                onClick={() => setActiveThreadId(message.id)}
              >
                <div className="avatar">{message.sender_name.slice(0, 1)}</div>
                <div className="message-body">
                  <div className="meta">
                    <strong>{message.sender_name}</strong>
                    <span>{message.sender_role}</span>
                    <time>{formatTime(message.created_at)}</time>
                    {linkedTask && (
                      <mark>
                        <CheckCircle2 size={14} /> #{linkedTask.number} · {linkedTask.status.replace("_", " ")}
                      </mark>
                    )}
                  </div>
                  <p>{firstLines(message.body)}</p>
                  {linkedTask && (
                    <div className="message-task-line">
                      <span>{linkedTask.assignee_name || "unassigned"}</span>
                      <span>updated {formatTime(linkedTask.updated_at)}</span>
                    </div>
                  )}
                  {(mentionedAgents.length > 0 || messageWorkItems.length > 0) && (
                    <div className="agent-mention-line">
                      {mentionedAgents.map((agent) => (
                        <span key={agent.id}>@{agent.handle}</span>
                      ))}
                      {messageWorkItems.map((item) => (
                        <strong key={item.id}>@{item.agent_handle} {item.status}</strong>
                      ))}
                    </div>
                  )}
                  <div className="message-actions">
                    <button className="reply-pill"><MessageSquare size={15} /> Open thread</button>
                    <button
                      className={`follow-pill ${message.thread_followed ? "active" : ""}`}
                      onClick={(event) => {
                        event.stopPropagation();
                        toggleThreadFollow(message);
                      }}
                    >
                      {message.thread_followed ? "Following" : "Muted"}
                    </button>
                  </div>
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
              <p>Create a task above or use “Send Task” in the channel composer.</p>
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

      <footer className="composer">
        <div className="composer-label">
          <button type="button" disabled={agents.length === 0 || !channel} onClick={openMentionPicker}>Add Agent</button>
          <span>{agents.length === 0 ? "Add an agent before assigning work." : "Use @ to assign work to an agent in this channel."}</span>
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
          value={draft}
          onChange={(event) => {
            setDraft(event.target.value);
            refreshMentionState(event.target.value, event.target.selectionStart);
          }}
          onSelect={(event) => refreshMentionState(draft, event.currentTarget.selectionStart)}
          onKeyDown={handleComposerKeyDown}
          disabled={!channel}
          placeholder={channel ? `Message #${channel.name}; type @ or Add Agent to call an agent` : "Create a channel before messaging"}
        />
        <div className="composer-actions">
          <div className="send-mode" aria-label="Send mode">
            <button className={!sendAsTask ? "active" : ""} onClick={() => setSendAsTask(false)}>Message</button>
            <button className={sendAsTask ? "active" : ""} onClick={() => setSendAsTask(true)}>Task</button>
          </div>
          <span className="composer-hint">Enter to send · Shift+Enter for newline</span>
          <button className="send" disabled={!channel || !draft.trim()} onClick={() => sendRootMessage(sendAsTask)}>
            Send <Send size={15} />
          </button>
        </div>
      </footer>
    </section>
  );
}
