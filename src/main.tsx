import React, { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import {
  AtSign,
  Bot,
  CheckCircle2,
  ChevronDown,
  Circle,
  Hash,
  LayoutList,
  MessageSquare,
  Plus,
  Reply,
  Save,
  Search,
  Send,
  Settings,
  Sparkles,
  Square,
  Trash2,
  Users,
  X,
} from "lucide-react";
import "./styles.css";

type Agent = {
  id: string;
  handle: string;
  display_name: string;
  role: string;
  status: string;
  runtime: string;
  model: string;
  avatar: string;
  description: string;
  launch_command: string;
  working_directory: string;
};

type Channel = {
  id: string;
  name: string;
  description: string;
  unread_count: number;
};

type Message = {
  id: string;
  channel_id: string;
  thread_root_id: string | null;
  sender_name: string;
  sender_role: string;
  body: string;
  is_task: boolean;
  task_number: number | null;
  task_status: string | null;
  created_at: string;
};

type Task = {
  id: string;
  number: number;
  title: string;
  status: string;
  channel_name: string;
  assignee_id: string | null;
  assignee_name: string | null;
};

type AgentRun = {
  id: string;
  agent_id: string;
  agent_handle: string;
  command: string;
  working_directory: string;
  status: string;
  pid: number | null;
  exit_code: number | null;
  log: string;
  started_at: string;
  stopped_at: string | null;
};

type Bootstrap = {
  db_url: string;
  channels: Channel[];
  agents: Agent[];
  messages: Message[];
  tasks: Task[];
  agent_runs: AgentRun[];
};

type AgentForm = {
  handle: string;
  displayName: string;
  runtime: string;
  model: string;
  description: string;
  launchCommand: string;
  workingDirectory: string;
};

const EMPTY_AGENT_FORM: AgentForm = {
  handle: "",
  displayName: "",
  runtime: "codex",
  model: "gpt-5.5",
  description: "",
  launchCommand: "",
  workingDirectory: "",
};

const TASK_STATUSES = ["todo", "in_progress", "in_review", "done"] as const;
const ACTIVE_RUN_STATUSES = new Set(["starting", "running", "stopping"]);

function formatTime(value: string) {
  return new Intl.DateTimeFormat("en", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

function firstLines(text: string, lines = 8) {
  const split = text.trim().split("\n");
  return split.slice(0, lines).join("\n") + (split.length > lines ? "\n..." : "");
}

function App() {
  const [data, setData] = useState<Bootstrap | null>(null);
  const [activeChannelId, setActiveChannelId] = useState<string>("");
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<"chat" | "tasks">("chat");
  const [draft, setDraft] = useState("");
  const [replyDraft, setReplyDraft] = useState("");
  const [newChannel, setNewChannel] = useState("");
  const [channelNameDraft, setChannelNameDraft] = useState("");
  const [channelDescriptionDraft, setChannelDescriptionDraft] = useState("");
  const [agentDraft, setAgentDraft] = useState<AgentForm>(EMPTY_AGENT_FORM);
  const [editingAgentId, setEditingAgentId] = useState<string | null>(null);
  const [agentEdit, setAgentEdit] = useState<AgentForm>(EMPTY_AGENT_FORM);

  async function refresh() {
    const next = await invoke<Bootstrap>("bootstrap");
    setData(next);
    setActiveChannelId((prev) => {
      if (next.channels.some((item) => item.id === prev)) return prev;
      return next.channels[0]?.id || "";
    });
    setActiveThreadId((prev) => {
      if (prev && next.messages.some((item) => item.id === prev)) return prev;
      return next.messages.find((m) => !m.thread_root_id)?.id || null;
    });
  }

  async function mutate(command: string, args: Record<string, unknown> = {}) {
    await invoke(command, args);
    await refresh();
  }

  useEffect(() => {
    refresh().catch((err) => console.error(err));
  }, []);

  const channel = useMemo(() => {
    return data?.channels.find((c) => c.id === activeChannelId) ?? data?.channels[0] ?? null;
  }, [activeChannelId, data]);

  const rootMessages = useMemo(() => {
    if (!data || !channel) return [];
    return data.messages.filter((m) => m.channel_id === channel.id && !m.thread_root_id);
  }, [data, channel]);

  const activeRoot = activeThreadId ? rootMessages.find((m) => m.id === activeThreadId) ?? null : null;

  const replies = useMemo(() => {
    if (!data || !activeRoot) return [];
    return data.messages.filter((m) => m.thread_root_id === activeRoot.id);
  }, [data, activeRoot]);

  useEffect(() => {
    setChannelNameDraft(channel?.name ?? "");
    setChannelDescriptionDraft(channel?.description ?? "");
  }, [channel?.id, channel?.name, channel?.description]);

  async function createChannel() {
    const name = newChannel.trim().replace(/^#/, "");
    if (!name) return;
    await mutate("create_channel", { name });
    setNewChannel("");
  }

  async function saveChannel() {
    if (!channel || !channelNameDraft.trim()) return;
    await mutate("update_channel", {
      channelId: channel.id,
      name: channelNameDraft,
      description: channelDescriptionDraft,
    });
  }

  async function deleteChannel() {
    if (!channel) return;
    if (!window.confirm(`Delete #${channel.name} and its messages/tasks?`)) return;
    await mutate("delete_channel", { channelId: channel.id });
  }

  async function createAgent() {
    const handle = agentDraft.handle.trim().replace(/^@/, "");
    if (!handle) return;
    await mutate("create_agent", {
      handle,
      displayName: agentDraft.displayName || handle,
      runtime: agentDraft.runtime,
      model: agentDraft.model,
      launchCommand: agentDraft.launchCommand,
      workingDirectory: agentDraft.workingDirectory,
    });
    setAgentDraft(EMPTY_AGENT_FORM);
  }

  function startEditAgent(agent: Agent) {
    setEditingAgentId(agent.id);
    setAgentEdit({
      handle: agent.handle,
      displayName: agent.display_name,
      runtime: agent.runtime,
      model: agent.model,
      description: agent.description,
      launchCommand: agent.launch_command,
      workingDirectory: agent.working_directory,
    });
  }

  async function saveAgent() {
    if (!editingAgentId || !agentEdit.handle.trim()) return;
    await mutate("update_agent", {
      agentId: editingAgentId,
      handle: agentEdit.handle,
      displayName: agentEdit.displayName || agentEdit.handle,
      runtime: agentEdit.runtime,
      model: agentEdit.model,
      description: agentEdit.description,
      launchCommand: agentEdit.launchCommand,
      workingDirectory: agentEdit.workingDirectory,
    });
    setEditingAgentId(null);
    setAgentEdit(EMPTY_AGENT_FORM);
  }

  async function deleteAgent(agent: Agent) {
    if (!window.confirm(`Delete @${agent.handle}? Existing messages will keep their sender name.`)) return;
    await mutate("delete_agent", { agentId: agent.id });
    if (editingAgentId === agent.id) setEditingAgentId(null);
  }

  async function sendRootMessage(asTask = false) {
    if (!channel || !draft.trim()) return;
    await mutate("send_message", {
      channelId: channel.id,
      threadRootId: null,
      body: draft.trim(),
      asTask,
    });
    setDraft("");
  }

  async function sendReply() {
    if (!channel || !activeRoot || !replyDraft.trim()) return;
    await mutate("send_message", {
      channelId: channel.id,
      threadRootId: activeRoot.id,
      body: replyDraft.trim(),
      asTask: false,
    });
    setReplyDraft("");
  }

  async function updateTaskStatus(task: Task, status: string) {
    await mutate("update_task_status", { taskId: task.id, status });
  }

  async function claimTask(task: Task, agentId: string) {
    await mutate("claim_task", { taskId: task.id, agentId: agentId || null });
  }

  function activeRunFor(agentId: string) {
    return data?.agent_runs.find((run) => run.agent_id === agentId && ACTIVE_RUN_STATUSES.has(run.status)) ?? null;
  }

  async function startAgent(agent: Agent) {
    await mutate("start_agent", { agentId: agent.id });
  }

  async function stopAgent(run: AgentRun) {
    await mutate("stop_agent", { runId: run.id });
  }

  if (!data) {
    return <div className="boot">Opening LocalSlock...</div>;
  }

  return (
    <main className="app theme-liquid">
      <aside className="sidebar">
        <section className="workspace">
          <button className="workspace-switch">
            LocalSlock <ChevronDown size={16} />
          </button>
        </section>

        <nav className="rail">
          <button className="rail-item active"><MessageSquare size={18} /></button>
          <button className="rail-item"><Users size={18} /></button>
        </nav>

        <section className="quick-actions">
          <button><Search size={18} /> Search <span>⌘K</span></button>
          <button><MessageSquare size={18} /> Threads <strong>{rootMessages.length}</strong></button>
          <button><LayoutList size={18} /> Tasks <strong>{data.tasks.length}</strong></button>
          <button><Sparkles size={18} /> Agents <strong>{data.agents.length}</strong></button>
        </section>

        <section className="channel-block">
          <div className="section-title">
            <span><ChevronDown size={14} /> Channels {data.channels.length}</span>
            <button onClick={createChannel} title="Create channel"><Plus size={18} /></button>
          </div>
          <div className="new-channel">
            <input
              value={newChannel}
              onChange={(event) => setNewChannel(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") createChannel();
              }}
              placeholder="new-channel"
            />
          </div>
          {data.channels.map((item) => (
            <button
              key={item.id}
              className={`channel ${item.id === channel?.id ? "selected" : ""}`}
              onClick={() => {
                setActiveChannelId(item.id);
                const first = data.messages.find((m) => m.channel_id === item.id && !m.thread_root_id);
                setActiveThreadId(first?.id ?? null);
              }}
            >
              <Hash size={17} /> {item.name}
              {item.unread_count > 0 && <strong>{item.unread_count}</strong>}
            </button>
          ))}
          {data.channels.length === 0 && (
            <div className="empty-mini">Create a channel to start chatting.</div>
          )}
          {channel && (
            <div className="management-card">
              <h4>Channel Settings</h4>
              <input
                value={channelNameDraft}
                onChange={(event) => setChannelNameDraft(event.target.value)}
                placeholder="channel-name"
              />
              <textarea
                value={channelDescriptionDraft}
                onChange={(event) => setChannelDescriptionDraft(event.target.value)}
                placeholder="Channel description"
              />
              <div className="inline-actions">
                <button onClick={saveChannel}><Save size={15} /> Save</button>
                <button className="danger" onClick={deleteChannel}><Trash2 size={15} /> Delete</button>
              </div>
            </div>
          )}
        </section>

        <section className="agent-list">
          <div className="section-title"><span><ChevronDown size={14} /> Agents {data.agents.length}</span></div>
          <div className="agent-form">
            <input
              value={agentDraft.handle}
              onChange={(event) => setAgentDraft({ ...agentDraft, handle: event.target.value })}
              onKeyDown={(event) => {
                if (event.key === "Enter") createAgent();
              }}
              placeholder="@agent"
            />
            <input
              value={agentDraft.displayName}
              onChange={(event) => setAgentDraft({ ...agentDraft, displayName: event.target.value })}
              placeholder="display name"
            />
            <select
              value={agentDraft.runtime}
              onChange={(event) => setAgentDraft({ ...agentDraft, runtime: event.target.value })}
            >
              <option value="codex">Codex</option>
              <option value="claude">Claude</option>
              <option value="kimi">Kimi</option>
            </select>
            <input
              value={agentDraft.model}
              onChange={(event) => setAgentDraft({ ...agentDraft, model: event.target.value })}
              placeholder="model"
            />
            <textarea
              value={agentDraft.launchCommand}
              onChange={(event) => setAgentDraft({ ...agentDraft, launchCommand: event.target.value })}
              placeholder="launch command; empty uses a placeholder runtime"
            />
            <input
              value={agentDraft.workingDirectory}
              onChange={(event) => setAgentDraft({ ...agentDraft, workingDirectory: event.target.value })}
              placeholder="working directory"
            />
            <button onClick={createAgent}><Plus size={16} /> Add agent</button>
          </div>
          {data.agents.map((agent) => {
            const run = activeRunFor(agent.id);
            return (
              <div className="agent-card" key={agent.id}>
                <button className="agent" onClick={() => startEditAgent(agent)}>
                  <div className="avatar">{agent.avatar || "A"}</div>
                  <div>
                    <strong>{agent.display_name}</strong>
                    <span>@{agent.handle} · {agent.runtime} · {agent.status}</span>
                  </div>
                  <Circle className={`dot ${agent.status}`} size={10} />
                </button>
                <div className="agent-runtime-actions">
                  {run ? (
                    <button className="runtime-stop" onClick={() => stopAgent(run)}>
                      <Square size={14} /> Stop
                    </button>
                  ) : (
                    <button className="runtime-start" onClick={() => startAgent(agent)}>
                      <Sparkles size={14} /> Start
                    </button>
                  )}
                  <button className="icon-danger" onClick={() => deleteAgent(agent)} title="Delete agent">
                    <Trash2 size={15} />
                  </button>
                </div>
              </div>
            );
          })}
          {editingAgentId && (
            <div className="management-card">
              <h4>Edit Agent</h4>
              <input
                value={agentEdit.handle}
                onChange={(event) => setAgentEdit({ ...agentEdit, handle: event.target.value })}
                placeholder="@agent"
              />
              <input
                value={agentEdit.displayName}
                onChange={(event) => setAgentEdit({ ...agentEdit, displayName: event.target.value })}
                placeholder="display name"
              />
              <select
                value={agentEdit.runtime}
                onChange={(event) => setAgentEdit({ ...agentEdit, runtime: event.target.value })}
              >
                <option value="codex">Codex</option>
                <option value="claude">Claude</option>
                <option value="kimi">Kimi</option>
              </select>
              <input
                value={agentEdit.model}
                onChange={(event) => setAgentEdit({ ...agentEdit, model: event.target.value })}
                placeholder="model"
              />
              <textarea
                value={agentEdit.launchCommand}
                onChange={(event) => setAgentEdit({ ...agentEdit, launchCommand: event.target.value })}
                placeholder="launch command; empty uses a placeholder runtime"
              />
              <input
                value={agentEdit.workingDirectory}
                onChange={(event) => setAgentEdit({ ...agentEdit, workingDirectory: event.target.value })}
                placeholder="working directory"
              />
              <textarea
                value={agentEdit.description}
                onChange={(event) => setAgentEdit({ ...agentEdit, description: event.target.value })}
                placeholder="Agent notes"
              />
              <div className="inline-actions">
                <button onClick={saveAgent}><Save size={15} /> Save</button>
                <button onClick={() => setEditingAgentId(null)}><X size={15} /> Cancel</button>
              </div>
            </div>
          )}
          {data.agents.length === 0 && (
            <div className="empty-mini">Add a local agent profile first.</div>
          )}
        </section>

        <section className="profile">
          <div className="avatar human">D</div>
          <div>
            <strong>Dylan</strong>
            <span>local owner</span>
          </div>
          <button><Settings size={18} /></button>
        </section>
      </aside>

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
            <button className="style-pill"><Sparkles size={16} /> Liquid Glass</button>
            <button><Square size={16} /></button>
            <button><Settings size={16} /></button>
            <button><Users size={16} /> {data.agents.length + 1}</button>
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
            {rootMessages.map((message) => (
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
                    {message.task_number && <mark><CheckCircle2 size={14} /> #{message.task_number}</mark>}
                  </div>
                  <p>{firstLines(message.body)}</p>
                  <button className="reply-pill"><MessageSquare size={15} /> Open thread</button>
                </div>
              </article>
            ))}
          </div>
        ) : (
          <div className="task-board">
            {data.tasks.length === 0 && (
              <div className="empty-state">
                <LayoutList size={34} />
                <h2>No tasks yet</h2>
                <p>Use “Send Task” in a channel to create the first local task.</p>
              </div>
            )}
            {data.tasks.map((task) => (
              <article className="task-card" key={task.id}>
                <span>#{task.number}</span>
                <h3>{task.title}</h3>
                <p>{task.channel_name} · {task.assignee_name || "unassigned"}</p>
                <div className="task-controls">
                  <select value={task.assignee_id ?? ""} onChange={(event) => claimTask(task, event.target.value)}>
                    <option value="">Unassigned</option>
                    {data.agents.map((agent) => (
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

      <aside className="thread">
        <header>
          <div>
            <h2>Thread <span>{channel ? `- #${channel.name}` : "- no channel"}</span></h2>
            <p>{activeRoot ? `Root ${activeRoot.id.slice(0, 8)}` : "No thread selected"}</p>
          </div>
          <button onClick={() => setActiveThreadId(null)}><X size={18} /></button>
        </header>

        <section className="context-card">
          <h3>Local Context</h3>
          <div>
            <span>Channels</span>
            <strong>{data.channels.length}</strong>
          </div>
          <div>
            <span>Agents</span>
            <strong>{data.agents.length}</strong>
          </div>
          <div>
            <span>Tasks</span>
            <strong>{data.tasks.length}</strong>
          </div>
        </section>

        <section className="runtime-panel">
          <h3>Runtime Runs</h3>
          {data.agent_runs.length === 0 && (
            <p className="empty-mini">Start an agent to create the first local run log.</p>
          )}
          {data.agent_runs.slice(0, 5).map((run) => (
            <article key={run.id} className={`run-card ${run.status}`}>
              <div className="run-head">
                <strong>@{run.agent_handle}</strong>
                <span>{run.status}</span>
              </div>
              <code>{run.command}</code>
              <small>
                {formatTime(run.started_at)}
                {run.pid ? ` · pid ${run.pid}` : ""}
                {run.exit_code !== null ? ` · exit ${run.exit_code}` : ""}
              </small>
              {run.log && <pre>{run.log.trim().split("\n").slice(-8).join("\n")}</pre>}
            </article>
          ))}
        </section>

        {activeRoot && (
          <article className="thread-root">
            <div className="meta">
              <strong>{activeRoot.sender_name}</strong>
              <time>{formatTime(activeRoot.created_at)}</time>
            </div>
            <p>{activeRoot.body}</p>
          </article>
        )}

        <section className="reply-list">
          {!activeRoot && (
            <div className="empty-state compact">
              <MessageSquare size={28} />
              <h2>No thread selected</h2>
              <p>Select a root message after you create one.</p>
            </div>
          )}
          {replies.map((reply) => (
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
          ))}
        </section>

        <section className="reply-composer">
          <textarea
            value={replyDraft}
            onChange={(event) => setReplyDraft(event.target.value)}
            disabled={!activeRoot}
            placeholder={activeRoot ? "Reply in thread" : "Select a thread to reply"}
          />
          <button disabled={!activeRoot || !replyDraft.trim()} onClick={sendReply}>
            Reply <Reply size={15} />
          </button>
        </section>

        <section className="db-card">
          <Bot size={18} />
          <div>
            <strong>Postgres State</strong>
            <span>{data.db_url.replace(/:[^:@/]+@/, ":***@")}</span>
          </div>
        </section>
      </aside>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
