import {
  CheckCircle2,
  Hash,
  LayoutList,
  MessageSquare,
  PanelRight,
  Plus,
  Send,
} from "lucide-react";
import { useRef, useState, type KeyboardEvent } from "react";
import { useMentionPicker } from "../hooks/useMentionPicker";
import {
  mentionedAgentsForBody,
} from "../mentions";
import { Agent, AgentWorkItem, Channel, Message, TASK_STATUSES, Task } from "../types";
import { firstLines, formatTime } from "../ui-utils";
import { MessageMarkdown } from "./MessageMarkdown";

type ConversationProps = {
  channel: Channel | null;
  agents: Agent[];
  channelAgents: Agent[];
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
  openChannelAgentsModal: () => void;
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
  channelAgents,
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
  openChannelAgentsModal,
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
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
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

  function handleComposerKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (handleMentionKeyDown(event)) return;
    if (event.key === "Enter" && !event.shiftKey) {
      event.preventDefault();
      submitComposer();
    }
  }

  function submitComposer() {
    if (!channel || !draft.trim()) return;
    sendRootMessage(sendAsTask);
    closeMentionPicker();
    focusComposer();
  }
  return (
    <section className="conversation">
      <header className="topbar">
        <div className="channel-title">
          <span className="hash-card"><Hash /></span>
          <div>
            <h1>{channel?.name || "No channel"}</h1>
            <p>{channel?.description || "Create a channel from the sidebar"}</p>
            {channel && (
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
                  <MessageMarkdown body={firstLines(message.body)} />
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
          placeholder={channel ? `Message #${channel.name} - type @ to send to an agent` : "Create a channel before messaging"}
        />
        <div className="composer-actions">
          <div className="send-mode" aria-label="Send mode">
            <button className={!sendAsTask ? "active" : ""} onClick={() => setSendAsTask(false)}>Message</button>
            <button className={sendAsTask ? "active" : ""} onClick={() => setSendAsTask(true)}>Task</button>
          </div>
          <span className="composer-hint">Enter to send · Shift+Enter for newline</span>
          <button className="send" disabled={!channel || !draft.trim()} onClick={submitComposer}>
            Send <Send size={15} />
          </button>
        </div>
      </footer>
    </section>
  );
}
