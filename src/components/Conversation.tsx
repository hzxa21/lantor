import {
  CheckCircle2,
  Hash,
  LayoutList,
  MessageSquare,
  Paperclip,
  Plus,
  Send,
  X,
} from "lucide-react";
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useMentionPicker } from "../hooks/useMentionPicker";
import { isImeComposing } from "../input-utils";
import { Agent, Artifact, Channel, DraftAttachment, Message, TASK_STATUSES, Task } from "../types";
import { firstLines, formatTime } from "../ui-utils";
import { AgentAvatar } from "./AgentAvatar";
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
  openArtifact: (artifact: Artifact) => void;
};

function wasEdited(message: Message) {
  const created = new Date(message.created_at).getTime();
  const updated = new Date(message.updated_at).getTime();
  return Number.isFinite(created) && Number.isFinite(updated) && updated - created > 1000;
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
  openArtifact,
}: ConversationProps) {
  const [sendAsTask, setSendAsTask] = useState(false);
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

  useEffect(() => {
    if (!isDm) return;
    setSendAsTask(false);
    if (activeTab === "tasks") setActiveTab("chat");
  }, [activeTab, isDm, setActiveTab]);

  return (
    <section className="conversation">
      <header className="topbar">
        <div className="channel-title">
          <span className={`hash-card ${isDm ? "dm-card" : ""}`}>
            {isDm && dmAgent ? <AgentAvatar agent={dmAgent} size="sm" /> : <Hash />}
          </span>
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
        <div className="message-list">
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
                <div className="avatar">{message.sender_name.slice(0, 1)}</div>
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
          disabled={!channel}
          placeholder={
            channel
              ? isDm
                ? `Message @${dmAgent?.handle || "agent"}`
                : `Message #${channel.name} - type @ to send to an agent`
              : "Create a channel before messaging"
          }
        />
        {draftAttachments.length > 0 && (
          <div className="draft-attachments">
            {draftAttachments.map((attachment) => (
              <span key={attachment.id}>
                {attachment.original_name}
                <button
                  type="button"
                  onClick={() => removeDraftAttachment(attachment.id)}
                  aria-label={`Remove ${attachment.original_name}`}
                >
                  <X size={12} />
                </button>
              </span>
            ))}
          </div>
        )}
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
