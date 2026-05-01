import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { Conversation } from "./components/Conversation";
import { Modal } from "./components/Modal";
import { Sidebar } from "./components/Sidebar";
import { ThreadPanel } from "./components/ThreadPanel";
import {
  ACTIVE_RUN_STATUSES,
  Agent,
  AgentForm,
  AgentRun,
  AgentWorkItem,
  Bootstrap,
  EMPTY_AGENT_FORM,
  Message,
  RUNTIME_PRESETS,
  SearchResult,
  Task,
  modelOptionsForRuntime,
} from "./types";
import { buildPresetCommand, firstLines, formatTime } from "./ui-utils";
import "./styles.css";

const ACTIVITY_PHASE_LABELS: Record<string, string> = {
  thinking: "Thinking",
  acting: "Acting",
  tools: "Using tools",
  error: "Error",
  event: "Acting",
  message: "Acting",
  task: "Acting",
  event_error: "Error",
  run_error: "Error",
};

function phaseForActivity(kind: string) {
  return ACTIVITY_PHASE_LABELS[kind] ?? "Active";
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
  const [workAgentFilter, setWorkAgentFilter] = useState("");
  const [workStatusFilter, setWorkStatusFilter] = useState("active");
  const [showThread, setShowThread] = useState(true);
  const [showCreateChannelModal, setShowCreateChannelModal] = useState(false);
  const [showChannelSettingsModal, setShowChannelSettingsModal] = useState(false);
  const [showChannelAgentsModal, setShowChannelAgentsModal] = useState(false);
  const [showCreateAgentModal, setShowCreateAgentModal] = useState(false);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);

  async function refresh() {
    const next = await invoke<Bootstrap>("bootstrap");
    setData(next);
    setActiveChannelId((prev) => {
      if (next.channels.some((item) => item.id === prev)) return prev;
      return next.channels[0]?.id || "";
    });
    setActiveThreadId((prev) => {
      const rootIds = new Set(next.messages.filter((item) => !item.thread_root_id).map((item) => item.id));
      if (prev && rootIds.has(prev)) return prev;
      const repliedRootIds = new Set(next.messages.flatMap((item) => (item.thread_root_id ? [item.thread_root_id] : [])));
      return next.messages.find((item) => !item.thread_root_id && repliedRootIds.has(item.id))?.id || null;
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

  const threadedRootMessages = useMemo(() => {
    if (!data || !channel) return [];
    const repliedRootIds = new Set(
      data.messages
        .filter((message) => message.channel_id === channel.id && message.thread_root_id)
        .map((message) => message.thread_root_id),
    );
    return rootMessages.filter((message) => repliedRootIds.has(message.id));
  }, [data, channel, rootMessages]);

  const visibleTasks = useMemo(() => {
    if (!data || !channel) return [];
    return data.tasks.filter((task) => task.channel_id === channel.id);
  }, [data, channel]);

  const channelMemberIds = useMemo(() => {
    if (!data || !channel) return new Set<string>();
    return new Set(data.channel_members.filter((member) => member.channel_id === channel.id).map((member) => member.agent_id));
  }, [data, channel]);

  const channelAgents = useMemo(() => {
    if (!data || !channel) return [];
    return data.agents.filter((agent) => channelMemberIds.has(agent.id));
  }, [data, channel, channelMemberIds]);

  const activeTask = useMemo(() => {
    if (!data || !activeRoot) return null;
    return data.tasks.find((task) => task.message_id === activeRoot.id) ?? null;
  }, [data, activeRoot]);

  const visibleWorkItems = useMemo(() => {
    if (!data) return [];
    return data.agent_work_items.filter((item) => {
      if (workAgentFilter && item.agent_id !== workAgentFilter) return false;
      if (workStatusFilter === "active") {
        return ["queued", "running", "cancelling"].includes(item.status);
      }
      if (workStatusFilter === "finished") {
        return ["done", "failed", "cancelled"].includes(item.status);
      }
      if (workStatusFilter !== "all") return item.status === workStatusFilter;
      return true;
    });
  }, [data, workAgentFilter, workStatusFilter]);

  const queuedWorkItemCount = useMemo(() => {
    return data?.agent_work_items.filter((item) => item.status === "queued").length ?? 0;
  }, [data]);

  const followedThreads = useMemo(() => {
    return threadedRootMessages.filter((message) => message.thread_followed).length;
  }, [threadedRootMessages]);

  const selectedAgent = useMemo(() => {
    if (!data || !selectedAgentId) return null;
    return data.agents.find((agent) => agent.id === selectedAgentId) ?? null;
  }, [data, selectedAgentId]);

  const selectedAgentRun = useMemo(() => {
    if (!selectedAgent) return null;
    return activeRunFor(selectedAgent.id);
  }, [selectedAgent, data?.agent_runs]);

  const selectedAgentActivities = useMemo(() => {
    if (!data || !selectedAgent) return [];
    return data.agent_activities
      .filter((activity) => activity.agent_id === selectedAgent.id || activity.agent_handle === selectedAgent.handle)
      .slice(0, 24);
  }, [data, selectedAgent]);

  const selectedAgentLiveActivity = useMemo(() => {
    return selectedAgentActivities.find((activity) => activity.run_id === selectedAgentRun?.id)
      ?? selectedAgentActivities.find((activity) => activity.kind in ACTIVITY_PHASE_LABELS)
      ?? null;
  }, [selectedAgentActivities, selectedAgentRun]);

  const selectedAgentPhase = selectedAgent ? (selectedAgentRun
    ? {
        kind: selectedAgentLiveActivity?.kind ?? "run",
        label: selectedAgentLiveActivity ? phaseForActivity(selectedAgentLiveActivity.kind) : "Running",
        detail: selectedAgentLiveActivity?.detail ?? "Waiting for observable output from the agent.",
      }
    : {
        kind: selectedAgent.status,
        label: selectedAgent.status,
        detail: "No active run.",
      }) : null;

  const selectedAgentWorkItems = useMemo(() => {
    if (!data || !selectedAgent) return [];
    return data.agent_work_items.filter((item) => item.agent_id === selectedAgent.id).slice(0, 6);
  }, [data, selectedAgent]);

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

    const workItemHits = data.agent_work_items
      .filter((item) => `${item.agent_handle} ${item.status} ${item.title} ${item.context}`.toLowerCase().includes(query))
      .map((item) => ({
        id: item.id,
        kind: "work",
        title: item.title,
        detail: `${item.agent_handle} · ${item.status}`,
        channelId: item.channel_id,
        threadId: item.thread_root_id,
        agentId: item.agent_id,
      }));

    return [...channelHits, ...taskHits, ...messageHits, ...agentHits, ...workItemHits, ...activityHits].slice(0, 9);
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
    setShowCreateChannelModal(false);
  }

  async function saveChannel() {
    if (!channel || !channelNameDraft.trim()) return;
    await mutate("update_channel", {
      channelId: channel.id,
      name: channelNameDraft,
      description: channelDescriptionDraft,
    });
    setShowChannelSettingsModal(false);
  }

  async function deleteChannel() {
    if (!channel) return;
    if (!window.confirm(`Delete #${channel.name} and its messages/tasks?`)) return;
    await mutate("delete_channel", { channelId: channel.id });
    setShowChannelSettingsModal(false);
  }

  function selectChannel(channelId: string) {
    setActiveChannelId(channelId);
    const repliedRootIds = new Set(
      data?.messages
        .filter((message) => message.channel_id === channelId && message.thread_root_id)
        .map((message) => message.thread_root_id) ?? [],
    );
    const first = data?.messages.find((m) => m.channel_id === channelId && !m.thread_root_id && repliedRootIds.has(m.id));
    setActiveThreadId(first?.id ?? null);
  }

  async function setChannelMember(agentId: string, member: boolean) {
    if (!channel) return;
    await mutate("set_channel_agent_membership", {
      channelId: channel.id,
      agentId,
      member,
    });
  }

  async function createAgent() {
    const handle = agentDraft.handle.trim().replace(/^@/, "");
    if (!handle) return;
    const nextForm = {
      ...agentDraft,
      handle,
      displayName: agentDraft.displayName || handle,
      launchCommand: buildPresetCommand({ ...agentDraft, handle, displayName: agentDraft.displayName || handle }),
      workingDirectory: "",
    };
    const agentId = await invoke<string>("create_agent", {
      handle,
      displayName: nextForm.displayName,
      runtime: nextForm.runtime,
      model: nextForm.model,
      launchCommand: nextForm.launchCommand,
      workingDirectory: nextForm.workingDirectory,
    });
    if (channel) {
      await invoke("set_channel_agent_membership", {
        channelId: channel.id,
        agentId,
        member: true,
      });
    }
    await refresh();
    setAgentDraft(EMPTY_AGENT_FORM);
    setShowCreateAgentModal(false);
  }

  function updateDraftRuntime(runtime: string) {
    const preset = RUNTIME_PRESETS[runtime];
    const currentPreset = RUNTIME_PRESETS[agentDraft.runtime];
    const shouldReplaceModel =
      !agentDraft.model.trim() ||
      !preset?.models.includes(agentDraft.model) ||
      (currentPreset && agentDraft.model === currentPreset.defaultModel);
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
      !agentEdit.model.trim() ||
      !preset?.models.includes(agentEdit.model) ||
      (currentPreset && agentEdit.model === currentPreset.defaultModel);
    setAgentEdit({
      ...agentEdit,
      runtime,
      model: preset && shouldReplaceModel ? preset.defaultModel : agentEdit.model,
    });
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
    const handle = agentEdit.handle.trim().replace(/^@/, "");
    const nextForm = {
      ...agentEdit,
      handle,
      displayName: agentEdit.displayName || handle,
      launchCommand: buildPresetCommand({ ...agentEdit, handle, displayName: agentEdit.displayName || handle }),
      workingDirectory: "",
    };
    await mutate("update_agent", {
      agentId: editingAgentId,
      handle: nextForm.handle,
      displayName: nextForm.displayName || nextForm.handle,
      runtime: nextForm.runtime,
      model: nextForm.model,
      description: nextForm.description,
      launchCommand: nextForm.launchCommand,
      workingDirectory: nextForm.workingDirectory,
    });
    setEditingAgentId(null);
    setAgentEdit(EMPTY_AGENT_FORM);
  }

  function cancelEditAgent() {
    setEditingAgentId(null);
    setAgentEdit(EMPTY_AGENT_FORM);
  }

  async function deleteAgent(agent: Agent) {
    if (!window.confirm(`Delete @${agent.handle}? Existing messages will keep their sender name.`)) return;
    await mutate("delete_agent", { agentId: agent.id });
    if (editingAgentId === agent.id) setEditingAgentId(null);
    if (selectedAgentId === agent.id) setSelectedAgentId(null);
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

  function openWorkItem(item: AgentWorkItem) {
    if (item.channel_id) setActiveChannelId(item.channel_id);
    if (item.thread_root_id) {
      setActiveThreadId(item.thread_root_id);
      setActiveTab("chat");
    }
    const agent = data?.agents.find((candidate) => candidate.id === item.agent_id);
    if (agent) setSelectedAgentId(agent.id);
  }

  function openSearchResult(result: SearchResult) {
    if (result.agentId) {
      const agent = data?.agents.find((item) => item.id === result.agentId);
      if (agent) setSelectedAgentId(agent.id);
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

  async function cancelWorkItem(item: AgentWorkItem) {
    await mutate("cancel_agent_work", { workItemId: item.id });
  }

  async function retryWorkItem(item: AgentWorkItem) {
    await mutate("retry_agent_work", { workItemId: item.id });
  }

  async function installSupervisorService() {
    await mutate("install_supervisor_service");
  }

  async function uninstallSupervisorService() {
    await mutate("uninstall_supervisor_service");
  }

  if (!data) {
    return <div className="boot">Opening LocalSlock...</div>;
  }

  return (
    <main className={`app theme-liquid ${showThread ? "" : "thread-hidden"}`}>
      <Sidebar
        data={data}
        channel={channel}
        rootMessages={threadedRootMessages}
        followedThreads={followedThreads}
        searchQuery={searchQuery}
        searchResults={searchResults}
        setSearchQuery={setSearchQuery}
        openSearchResult={openSearchResult}
        openCreateChannelModal={() => setShowCreateChannelModal(true)}
        openChannelSettingsModal={() => setShowChannelSettingsModal(true)}
        selectChannel={selectChannel}
        activeThreadId={activeThreadId}
        setActiveThreadId={setActiveThreadId}
        openCreateAgentModal={() => setShowCreateAgentModal(true)}
        openAgentDetail={(agent) => setSelectedAgentId(agent.id)}
        activeRunFor={activeRunFor}
        startAgent={startAgent}
        stopAgent={stopAgent}
      />

      <Conversation
        channel={channel}
        agents={data.agents}
        channelAgents={channelAgents}
        activeTab={activeTab}
        activeRoot={activeRoot}
        rootMessages={rootMessages}
        visibleTasks={visibleTasks}
        workItems={data.agent_work_items}
        draft={draft}
        taskDraft={taskDraft}
        taskTitleDrafts={taskTitleDrafts}
        showThread={showThread}
        setActiveTab={setActiveTab}
        setActiveThreadId={setActiveThreadId}
        setShowThread={setShowThread}
        openChannelAgentsModal={() => setShowChannelAgentsModal(true)}
        taskForMessage={taskForMessage}
        toggleThreadFollow={toggleThreadFollow}
        setTaskTitleDraft={setTaskTitleDraft}
        saveTaskTitle={saveTaskTitle}
        claimTask={claimTask}
        updateTaskStatus={updateTaskStatus}
        openTask={openTask}
        setTaskDraft={setTaskDraft}
        createTaskFromBoard={createTaskFromBoard}
        setDraft={setDraft}
        sendRootMessage={sendRootMessage}
      />

      {showThread && (
        <ThreadPanel
          channel={channel}
          agents={data.agents}
          activeRoot={activeRoot}
          activeTask={activeTask}
          replies={replies}
          taskTitleDrafts={taskTitleDrafts}
          replyDraft={replyDraft}
          toggleThreadFollow={toggleThreadFollow}
          setActiveThreadId={setActiveThreadId}
          setTaskTitleDraft={setTaskTitleDraft}
          saveTaskTitle={saveTaskTitle}
          claimTask={claimTask}
          updateTaskStatus={updateTaskStatus}
          setReplyDraft={setReplyDraft}
          sendReply={sendReply}
        />
      )}

      <Modal
        open={showCreateChannelModal}
        title="Create Channel"
        onClose={() => setShowCreateChannelModal(false)}
      >
        <div className="modal-form">
          <label>
            <span>Channel name</span>
            <input
              autoFocus
              value={newChannel}
              onChange={(event) => setNewChannel(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") createChannel();
              }}
              placeholder="local-slock"
            />
          </label>
          <div className="modal-actions">
            <button onClick={() => setShowCreateChannelModal(false)}>Cancel</button>
            <button className="primary" disabled={!newChannel.trim()} onClick={createChannel}>Create</button>
          </div>
        </div>
      </Modal>

      <Modal
        open={showChannelSettingsModal}
        title={channel ? `#${channel.name} Settings` : "Channel Settings"}
        onClose={() => setShowChannelSettingsModal(false)}
        width={560}
      >
        {channel && (
          <div className="modal-form">
            <label>
              <span>Channel name</span>
              <input
                value={channelNameDraft}
                onChange={(event) => setChannelNameDraft(event.target.value)}
                placeholder="channel-name"
              />
            </label>
            <label>
              <span>Description</span>
              <textarea
                value={channelDescriptionDraft}
                onChange={(event) => setChannelDescriptionDraft(event.target.value)}
                placeholder="Channel description"
              />
            </label>
            <div className="member-editor modal-member-editor">
              <strong>Agent members</strong>
              {data.agents.length === 0 && <span>No agents yet.</span>}
              {data.agents.map((agent) => (
                <label key={agent.id}>
                  <input
                    type="checkbox"
                    checked={channelMemberIds.has(agent.id)}
                    onChange={(event) => setChannelMember(agent.id, event.target.checked)}
                  />
                  @{agent.handle}
                </label>
              ))}
            </div>
            <div className="modal-actions split">
              <button className="danger" onClick={deleteChannel}>Delete Channel</button>
              <div>
                <button onClick={() => setShowChannelSettingsModal(false)}>Cancel</button>
                <button className="primary" disabled={!channelNameDraft.trim()} onClick={saveChannel}>Save</button>
              </div>
            </div>
          </div>
        )}
      </Modal>

      <Modal
        open={showChannelAgentsModal}
        title={channel ? `Agents in #${channel.name}` : "Channel Agents"}
        onClose={() => setShowChannelAgentsModal(false)}
        width={560}
      >
        <div className="modal-form">
          <div className="member-editor modal-member-editor channel-agent-picker">
            {data.agents.length === 0 && <span>No agents yet. Create an agent first.</span>}
            {data.agents.map((agent) => (
              <label key={agent.id}>
                <input
                  type="checkbox"
                  checked={channelMemberIds.has(agent.id)}
                  onChange={(event) => setChannelMember(agent.id, event.target.checked)}
                />
                <span className="agent-pick-row">
                  <strong>@{agent.handle}</strong>
                  <small>{agent.display_name} · {agent.runtime} · {agent.status}</small>
                </span>
              </label>
            ))}
          </div>
          <div className="modal-actions split">
            <button
              onClick={() => {
                setShowChannelAgentsModal(false);
                setShowCreateAgentModal(true);
              }}
            >
              Create new agent
            </button>
            <button className="primary" onClick={() => setShowChannelAgentsModal(false)}>Done</button>
          </div>
        </div>
      </Modal>

      <Modal
        open={showCreateAgentModal}
        title="Add Agent"
        onClose={() => setShowCreateAgentModal(false)}
        width={680}
      >
        <div className="modal-form agent-modal-form">
          <div className="two-col">
            <label>
              <span>Handle</span>
              <input
                autoFocus
                value={agentDraft.handle}
                onChange={(event) => setAgentDraft({ ...agentDraft, handle: event.target.value })}
                placeholder="@agent"
              />
            </label>
            <label>
              <span>Display name</span>
              <input
                value={agentDraft.displayName}
                onChange={(event) => setAgentDraft({ ...agentDraft, displayName: event.target.value })}
                placeholder="display name"
              />
            </label>
          </div>
          <div className="two-col">
            <label>
              <span>Agent type</span>
              <select value={agentDraft.runtime} onChange={(event) => updateDraftRuntime(event.target.value)}>
                <option value="codex">Codex</option>
                <option value="claude">Claude</option>
                <option value="kimi">Kimi</option>
              </select>
            </label>
            <label>
              <span>Model</span>
              <select
                value={agentDraft.model}
                onChange={(event) => setAgentDraft({ ...agentDraft, model: event.target.value })}
              >
                {modelOptionsForRuntime(agentDraft.runtime, agentDraft.model).map((model) => (
                  <option key={model} value={model}>{model}</option>
                ))}
              </select>
            </label>
          </div>
          <div className="modal-actions">
            <button onClick={() => setShowCreateAgentModal(false)}>Cancel</button>
            <button className="primary" disabled={!agentDraft.handle.trim()} onClick={createAgent}>Add agent</button>
          </div>
        </div>
      </Modal>

      <Modal
        open={Boolean(editingAgentId)}
        title="Edit Agent"
        onClose={cancelEditAgent}
        width={700}
      >
        <div className="modal-form agent-modal-form">
          <div className="two-col">
            <label>
              <span>Handle</span>
              <input
                autoFocus
                value={agentEdit.handle}
                onChange={(event) => setAgentEdit({ ...agentEdit, handle: event.target.value })}
                placeholder="@agent"
              />
            </label>
            <label>
              <span>Display name</span>
              <input
                value={agentEdit.displayName}
                onChange={(event) => setAgentEdit({ ...agentEdit, displayName: event.target.value })}
                placeholder="display name"
              />
            </label>
          </div>
          <div className="two-col">
            <label>
              <span>Runtime</span>
              <select value={agentEdit.runtime} onChange={(event) => updateEditRuntime(event.target.value)}>
                <option value="codex">Codex</option>
                <option value="claude">Claude</option>
                <option value="kimi">Kimi</option>
              </select>
            </label>
            <label>
              <span>Model</span>
              <select
                value={agentEdit.model}
                onChange={(event) => setAgentEdit({ ...agentEdit, model: event.target.value })}
              >
                {modelOptionsForRuntime(agentEdit.runtime, agentEdit.model).map((model) => (
                  <option key={model} value={model}>{model}</option>
                ))}
              </select>
            </label>
          </div>
          <label>
            <span>Notes</span>
            <textarea
              value={agentEdit.description}
              onChange={(event) => setAgentEdit({ ...agentEdit, description: event.target.value })}
              placeholder="Agent notes"
            />
          </label>
          <div className="modal-actions">
            <button onClick={cancelEditAgent}>Cancel</button>
            <button className="primary" disabled={!agentEdit.handle.trim()} onClick={saveAgent}>Save</button>
          </div>
        </div>
      </Modal>

      <Modal
        open={Boolean(selectedAgent)}
        title={selectedAgent ? `@${selectedAgent.handle}` : "Agent"}
        onClose={() => setSelectedAgentId(null)}
        width={720}
      >
        {selectedAgent && (
          <div className="agent-detail">
            <section className="agent-detail-hero">
              <div className="avatar large">{selectedAgent.avatar || selectedAgent.handle.slice(0, 1).toUpperCase()}</div>
              <div>
                <h3>{selectedAgent.display_name}</h3>
                <p>@{selectedAgent.handle} · {selectedAgent.runtime} · {selectedAgent.model}</p>
                {selectedAgentPhase && (
                  <div className="agent-phase-line">
                    <span className={`phase-badge ${selectedAgentPhase.kind}`}>{selectedAgentPhase.label}</span>
                    <small>{selectedAgentPhase.detail}</small>
                  </div>
                )}
              </div>
              <span className={`status-badge ${selectedAgent.status}`}>{selectedAgent.status}</span>
            </section>
            <section className="detail-grid">
              <div>
                <span>Workspace</span>
                <code>{selectedAgent.working_directory || "Not configured"}</code>
              </div>
              <div>
                <span>Active run</span>
                <code>{selectedAgentRun ? `${selectedAgentRun.status}${selectedAgentRun.pid ? ` · pid ${selectedAgentRun.pid}` : ""}` : "No active run"}</code>
              </div>
              <div>
                <span>Role</span>
                <code>{selectedAgent.role || "agent"}</code>
              </div>
              <div>
                <span>Description</span>
                <code>{selectedAgent.description || "No notes"}</code>
              </div>
            </section>
            <section className="detail-section">
              <h4>Live activity</h4>
              {selectedAgentActivities.length === 0 && <p className="empty-mini">No activity yet.</p>}
              {selectedAgentActivities.map((activity) => (
                <article key={activity.id} className={`detail-activity ${activity.kind}`}>
                  <div className="detail-activity-head">
                    <strong>{activity.title}</strong>
                    <span className={`phase-badge ${activity.kind}`}>{phaseForActivity(activity.kind)}</span>
                  </div>
                  <span>{formatTime(activity.created_at)} · {activity.kind}</span>
                  <p>{activity.detail}</p>
                </article>
              ))}
            </section>
            <section className="detail-section">
              <h4>Work items</h4>
              {selectedAgentWorkItems.length === 0 && <p className="empty-mini">No work assigned yet.</p>}
              {selectedAgentWorkItems.map((item) => (
                <article key={item.id} className="detail-work" onClick={() => {
                  openWorkItem(item);
                  setSelectedAgentId(null);
                }}>
                  <strong>{item.title}</strong>
                  <span>{item.status}{item.task_number ? ` · task #${item.task_number}` : ""}</span>
                </article>
              ))}
            </section>
            <div className="modal-actions split">
              <button className="danger" onClick={() => deleteAgent(selectedAgent)}>Delete Agent</button>
              <div>
                {selectedAgentRun ? (
                  <button onClick={() => stopAgent(selectedAgentRun)}>Stop</button>
                ) : (
                  <button onClick={() => startAgent(selectedAgent)}>Start</button>
                )}
                <button
                  className="primary"
                  onClick={() => {
                    startEditAgent(selectedAgent);
                    setSelectedAgentId(null);
                  }}
                >
                  Edit
                </button>
              </div>
            </div>
          </div>
        )}
      </Modal>
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
