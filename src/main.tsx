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
  thread_followed: boolean;
  task_number: number | null;
  task_status: string | null;
  created_at: string;
};

type Task = {
  id: string;
  number: number;
  message_id: string;
  channel_id: string;
  title: string;
  status: string;
  channel_name: string;
  assignee_id: string | null;
  assignee_name: string | null;
  created_at: string;
  updated_at: string;
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

type AgentActivity = {
  id: string;
  agent_id: string | null;
  agent_handle: string;
  run_id: string | null;
  kind: string;
  title: string;
  detail: string;
  created_at: string;
};

type SupervisorStatus = {
  pid: number | null;
  status: string;
  updated_at: string | null;
};

type LaunchAgentStatus = {
  label: string;
  plist_path: string;
  installed: boolean;
  loaded: boolean;
};

type Bootstrap = {
  db_url: string;
  channels: Channel[];
  agents: Agent[];
  messages: Message[];
  tasks: Task[];
  agent_runs: AgentRun[];
  agent_activities: AgentActivity[];
  supervisor: SupervisorStatus;
  launch_agent: LaunchAgentStatus;
};

type SearchResult = {
  id: string;
  kind: string;
  title: string;
  detail: string;
  channelId: string | null;
  threadId: string | null;
  agentId: string | null;
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

const RUNTIME_PRESETS: Record<string, { label: string; defaultModel: string; commandName: string }> = {
  codex: {
    label: "Codex",
    defaultModel: "gpt-5.5",
    commandName: "codex",
  },
  claude: {
    label: "Claude",
    defaultModel: "sonnet",
    commandName: "claude",
  },
  kimi: {
    label: "Kimi",
    defaultModel: "kimi-k2",
    commandName: "kimi",
  },
};

function shellQuote(value: string) {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

function presetPrompt(form: AgentForm) {
  const name = form.displayName || form.handle || "$LOCAL_SLOCK_AGENT_HANDLE";
  return [
    `You are ${name}, a local agent running inside LocalSlock.`,
    "You collaborate with one local human through channels, threads, and tasks.",
    "When you need to write back to LocalSlock, print exactly one stdout line beginning with LOCAL_SLOCK_EVENT followed by JSON.",
    "Supported JSON events:",
    '{"type":"message","channel":"local-slock","body":"..."}',
    '{"type":"message","channel":"local-slock","thread_root_id":"uuid","body":"..."}',
    '{"type":"message","channel":"local-slock","body":"...","as_task":true}',
    '{"type":"task_status","task_number":1,"status":"in_review"}',
    '{"type":"task_claim","task_number":1}',
    "Do not wrap LOCAL_SLOCK_EVENT lines in markdown.",
    "Use normal stdout for reasoning/logs only when you do not want to create LocalSlock state.",
  ].join("\n");
}

function buildPresetCommand(form: AgentForm) {
  const preset = RUNTIME_PRESETS[form.runtime];
  if (!preset) return "";
  const model = form.model.trim() || preset.defaultModel;
  const prompt = shellQuote(presetPrompt(form));
  const quotedModel = shellQuote(model);

  if (form.runtime === "codex") {
    return `LOCAL_SLOCK_PROMPT=${prompt}\n${preset.commandName} exec --model ${quotedModel} "$LOCAL_SLOCK_PROMPT"`;
  }
  if (form.runtime === "claude") {
    return `LOCAL_SLOCK_PROMPT=${prompt}\n${preset.commandName} -p "$LOCAL_SLOCK_PROMPT" --model ${quotedModel}`;
  }
  if (form.runtime === "kimi") {
    return `LOCAL_SLOCK_PROMPT=${prompt}\n${preset.commandName} --prompt "$LOCAL_SLOCK_PROMPT" --model ${quotedModel}`;
  }
  return "";
}

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
  const [taskDraft, setTaskDraft] = useState("");
  const [taskTitleDrafts, setTaskTitleDrafts] = useState<Record<string, string>>({});
  const [searchQuery, setSearchQuery] = useState("");
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

  useEffect(() => {
    const timer = window.setInterval(() => {
      refresh().catch((err) => console.error(err));
    }, 1500);
    return () => window.clearInterval(timer);
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

  const visibleTasks = useMemo(() => {
    if (!data || !channel) return [];
    return data.tasks.filter((task) => task.channel_id === channel.id);
  }, [data, channel]);

  const activeTask = useMemo(() => {
    if (!data || !activeRoot) return null;
    return data.tasks.find((task) => task.message_id === activeRoot.id) ?? null;
  }, [data, activeRoot]);

  const followedThreads = useMemo(() => {
    return rootMessages.filter((message) => message.thread_followed).length;
  }, [rootMessages]);

  const searchResults = useMemo(() => {
    if (!data) return [];
    const query = searchQuery.trim().toLowerCase();
    if (!query) return [];

    const channelHits = data.channels
      .filter((item) => `${item.name} ${item.description}`.toLowerCase().includes(query))
      .map((item) => ({
        id: item.id,
        kind: "channel",
        title: `#${item.name}`,
        detail: item.description || "channel",
        channelId: item.id,
        threadId: null,
        agentId: null,
      }));

    const taskHits = data.tasks
      .filter((item) => `${item.title} ${item.status} ${item.channel_name} ${item.assignee_name ?? ""}`.toLowerCase().includes(query))
      .map((item) => ({
        id: item.id,
        kind: "task",
        title: `#${item.number} ${item.title}`,
        detail: `${item.channel_name} · ${item.status.replace("_", " ")}`,
        channelId: item.channel_id,
        threadId: item.message_id,
        agentId: null,
      }));

    const messageHits = data.messages
      .filter((item) => `${item.sender_name} ${item.body}`.toLowerCase().includes(query))
      .map((item) => ({
        id: item.id,
        kind: item.thread_root_id ? "reply" : "message",
        title: firstLines(item.body, 1),
        detail: `${item.sender_name} · ${formatTime(item.created_at)}`,
        channelId: item.channel_id,
        threadId: item.thread_root_id ?? item.id,
        agentId: null,
      }));

    const agentHits = data.agents
      .filter((item) => `${item.handle} ${item.display_name} ${item.runtime} ${item.model} ${item.description}`.toLowerCase().includes(query))
      .map((item) => ({
        id: item.id,
        kind: "agent",
        title: `@${item.handle}`,
        detail: `${item.runtime} · ${item.model}`,
        channelId: null,
        threadId: null,
        agentId: item.id,
      }));

    const activityHits = data.agent_activities
      .filter((item) => `${item.agent_handle} ${item.kind} ${item.title} ${item.detail}`.toLowerCase().includes(query))
      .map((item) => ({
        id: item.id,
        kind: "activity",
        title: item.title,
        detail: `${item.agent_handle || "unknown"} · ${formatTime(item.created_at)}`,
        channelId: null,
        threadId: null,
        agentId: item.agent_id,
      }));

    return [...channelHits, ...taskHits, ...messageHits, ...agentHits, ...activityHits].slice(0, 9);
  }, [data, searchQuery]);

  function taskForMessage(messageId: string) {
    return data?.tasks.find((task) => task.message_id === messageId) ?? null;
  }

  useEffect(() => {
    setChannelNameDraft(channel?.name ?? "");
    setChannelDescriptionDraft(channel?.description ?? "");
  }, [channel?.id, channel?.name, channel?.description]);

  useEffect(() => {
    if (!activeChannelId) return;
    invoke("mark_channel_read", { channelId: activeChannelId }).catch((err) => console.error(err));
  }, [activeChannelId, data?.messages.length]);

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

  function updateDraftRuntime(runtime: string) {
    const preset = RUNTIME_PRESETS[runtime];
    const currentPreset = RUNTIME_PRESETS[agentDraft.runtime];
    const shouldReplaceModel =
      !agentDraft.model.trim() || (currentPreset && agentDraft.model === currentPreset.defaultModel);
    setAgentDraft({
      ...agentDraft,
      runtime,
      model: preset && shouldReplaceModel ? preset.defaultModel : agentDraft.model,
    });
  }

  function updateEditRuntime(runtime: string) {
    const preset = RUNTIME_PRESETS[runtime];
    const currentPreset = RUNTIME_PRESETS[agentEdit.runtime];
    const shouldReplaceModel =
      !agentEdit.model.trim() || (currentPreset && agentEdit.model === currentPreset.defaultModel);
    setAgentEdit({
      ...agentEdit,
      runtime,
      model: preset && shouldReplaceModel ? preset.defaultModel : agentEdit.model,
    });
  }

  function applyDraftPreset() {
    const command = buildPresetCommand(agentDraft);
    if (!command) return;
    setAgentDraft({ ...agentDraft, launchCommand: command });
  }

  function applyEditPreset() {
    const command = buildPresetCommand(agentEdit);
    if (!command) return;
    setAgentEdit({ ...agentEdit, launchCommand: command });
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

  async function createTaskFromBoard() {
    if (!channel || !taskDraft.trim()) return;
    await mutate("send_message", {
      channelId: channel.id,
      threadRootId: null,
      body: taskDraft.trim(),
      asTask: true,
    });
    setTaskDraft("");
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

  async function saveTaskTitle(task: Task) {
    const title = (taskTitleDrafts[task.id] ?? task.title).trim();
    if (!title || title === task.title) return;
    await mutate("update_task_title", { taskId: task.id, title });
    setTaskTitleDrafts((current) => {
      const next = { ...current };
      delete next[task.id];
      return next;
    });
  }

  function setTaskTitleDraft(task: Task, title: string) {
    setTaskTitleDrafts((current) => ({ ...current, [task.id]: title }));
  }

  async function claimTask(task: Task, agentId: string) {
    await mutate("claim_task", { taskId: task.id, agentId: agentId || null });
  }

  function openTask(task: Task) {
    setActiveChannelId(task.channel_id);
    setActiveThreadId(task.message_id);
    setActiveTab("chat");
  }

  function openSearchResult(result: SearchResult) {
    if (result.agentId) {
      const agent = data?.agents.find((item) => item.id === result.agentId);
      if (agent) startEditAgent(agent);
    }
    if (result.channelId) setActiveChannelId(result.channelId);
    if (result.threadId) {
      setActiveThreadId(result.threadId);
      setActiveTab("chat");
    }
  }

  async function toggleThreadFollow(message: Message) {
    await mutate("update_thread_followed", {
      threadRootId: message.id,
      followed: !message.thread_followed,
    });
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

  async function installSupervisorService() {
    await mutate("install_supervisor_service");
  }

  async function uninstallSupervisorService() {
    await mutate("uninstall_supervisor_service");
  }

  const draftPresetCommand = buildPresetCommand(agentDraft);
  const editPresetCommand = buildPresetCommand(agentEdit);

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
          <label className="search-box">
            <Search size={18} />
            <input
              value={searchQuery}
              onChange={(event) => setSearchQuery(event.target.value)}
              placeholder="Search local state"
            />
          </label>
          <button><MessageSquare size={18} /> Threads <strong>{followedThreads}/{rootMessages.length}</strong></button>
          <button><LayoutList size={18} /> Tasks <strong>{data.tasks.length}</strong></button>
          <button><Sparkles size={18} /> Agents <strong>{data.agents.length}</strong></button>
          {searchQuery.trim() && (
            <div className="search-results">
              {searchResults.length === 0 && <span>No local results</span>}
              {searchResults.map((result) => (
                <button key={`${result.kind}-${result.id}`} onClick={() => openSearchResult(result)}>
                  <strong>{result.kind}</strong>
                  <span>{result.title}</span>
                  <small>{result.detail}</small>
                </button>
              ))}
            </div>
          )}
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
              onChange={(event) => updateDraftRuntime(event.target.value)}
            >
              <option value="codex">Codex</option>
              <option value="claude">Claude</option>
              <option value="kimi">Kimi</option>
              <option value="custom">Custom</option>
            </select>
            <input
              value={agentDraft.model}
              onChange={(event) => setAgentDraft({ ...agentDraft, model: event.target.value })}
              placeholder="model"
            />
            <div className="preset-panel">
              <div>
                <strong>{RUNTIME_PRESETS[agentDraft.runtime]?.label ?? "Custom"} preset</strong>
                <span>
                  {draftPresetCommand
                    ? "Generate an editable launch command with the LocalSlock event protocol."
                    : "Custom runtime uses the command exactly as written."}
                </span>
              </div>
              {draftPresetCommand && <pre>{firstLines(draftPresetCommand, 6)}</pre>}
              <button disabled={!draftPresetCommand} onClick={applyDraftPreset}>
                <Sparkles size={14} /> Apply preset
              </button>
            </div>
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
                onChange={(event) => updateEditRuntime(event.target.value)}
              >
                <option value="codex">Codex</option>
                <option value="claude">Claude</option>
                <option value="kimi">Kimi</option>
                <option value="custom">Custom</option>
              </select>
              <input
                value={agentEdit.model}
                onChange={(event) => setAgentEdit({ ...agentEdit, model: event.target.value })}
                placeholder="model"
              />
              <div className="preset-panel">
                <div>
                  <strong>{RUNTIME_PRESETS[agentEdit.runtime]?.label ?? "Custom"} preset</strong>
                  <span>
                    {editPresetCommand
                      ? "Regenerate the command from current handle/model/runtime."
                      : "Custom runtime uses the command exactly as written."}
                  </span>
                </div>
                {editPresetCommand && <pre>{firstLines(editPresetCommand, 6)}</pre>}
                <button disabled={!editPresetCommand} onClick={applyEditPreset}>
                  <Sparkles size={14} /> Apply preset
                </button>
              </div>
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
          {activeRoot && (
            <button onClick={() => toggleThreadFollow(activeRoot)}>
              {activeRoot.thread_followed ? "Following" : "Muted"}
            </button>
          )}
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
          <div className="runtime-title">
            <h3>Runtime Runs</h3>
            <span className={`supervisor-chip ${data.supervisor.status}`}>
              supervisor {data.supervisor.status}
              {data.supervisor.pid ? ` · ${data.supervisor.pid}` : ""}
            </span>
          </div>
          <div className="service-card">
            <div>
              <strong>LaunchAgent</strong>
              <span>
                {data.launch_agent.installed ? "installed" : "not installed"} ·{" "}
                {data.launch_agent.loaded ? "loaded" : "not loaded"}
              </span>
              <code>{data.launch_agent.plist_path}</code>
            </div>
            <div className="service-actions">
              <button onClick={installSupervisorService}>
                <Sparkles size={14} /> Install
              </button>
              <button className="danger" onClick={uninstallSupervisorService}>
                <Trash2 size={14} /> Uninstall
              </button>
            </div>
          </div>
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

        <section className="activity-panel">
          <div className="activity-title">
            <h3>Agent Activity</h3>
            <span>{data.agent_activities.length}</span>
          </div>
          {data.agent_activities.length === 0 && (
            <p className="empty-mini">Agent activity appears here after profile edits, run lifecycle changes, and stdout events.</p>
          )}
          {data.agent_activities.slice(0, 12).map((activity) => (
            <article key={activity.id} className={`activity-card ${activity.kind}`}>
              <div className="activity-icon">
                {activity.agent_handle.slice(0, 1).toUpperCase() || "A"}
              </div>
              <div>
                <div className="activity-meta">
                  <strong>{activity.title}</strong>
                  <span>{formatTime(activity.created_at)}</span>
                </div>
                <p>{activity.detail || activity.kind}</p>
                <small>@{activity.agent_handle || "unknown"} · {activity.kind}</small>
              </div>
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
