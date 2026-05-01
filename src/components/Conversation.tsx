import {
  AtSign,
  CheckCircle2,
  Hash,
  LayoutList,
  MessageSquare,
  Plus,
  Send,
  Settings,
  Sparkles,
  Square,
  Users,
} from "lucide-react";
import { Agent, Channel, Message, TASK_STATUSES, Task } from "../types";
import { firstLines, formatTime } from "../ui-utils";

type ConversationProps = {
  channel: Channel | null;
  agents: Agent[];
  activeTab: "chat" | "tasks";
  activeRoot: Message | null;
  rootMessages: Message[];
  visibleTasks: Task[];
  draft: string;
  taskDraft: string;
  taskTitleDrafts: Record<string, string>;
  setActiveTab: (tab: "chat" | "tasks") => void;
  setActiveThreadId: (threadId: string | null) => void;
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
  draft,
  taskDraft,
  taskTitleDrafts,
  setActiveTab,
  setActiveThreadId,
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
          <button className="style-pill"><Sparkles size={16} /> Liquid Class</button>
          <button><Square size={16} /></button>
          <button><Settings size={16} /></button>
          <button><Users size={16} /> {agents.length + 1}</button>
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
        <textarea
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
          disabled={!channel}
          placeholder={channel ? `Root message in #${channel.name}` : "Create a channel before messaging"}
        />
        <div className="composer-actions">
          <button className="icon"><AtSign size={18} /></button>
          <button className="send" disabled={!channel} onClick={() => sendRootMessage(false)}>
            Send <Send size={15} />
          </button>
          <button className="task-send" disabled={!channel} onClick={() => sendRootMessage(true)}>Send Task</button>
        </div>
      </footer>
    </section>
  );
}
