import { useCallback, useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { AgentDetailDrawer } from "./components/AgentDetailDrawer";
import { AgentFormModal } from "./components/AgentFormModal";
import { ChannelAgentsModal } from "./components/ChannelAgentsModal";
import { ChannelSettingsModal } from "./components/ChannelSettingsModal";
import { Conversation } from "./components/Conversation";
import { CreateChannelModal } from "./components/CreateChannelModal";
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
  RuntimeCheck,
  SearchResult,
  Task,
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

function isTextInput(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || target.isContentEditable;
}

function errorMessage(err: unknown, fallback: string) {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === "string" && err.trim()) return err;
  return fallback;
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
  const [editingMessage, setEditingMessage] = useState<Message | null>(null);
  const [messageEditDraft, setMessageEditDraft] = useState("");
  const [appError, setAppError] = useState<string | null>(null);
  const [runtimeChecks, setRuntimeChecks] = useState<Record<string, RuntimeCheck>>({});

  async function refreshRuntimeChecks() {
    const entries = await Promise.all(
      Object.keys(RUNTIME_PRESETS).map(async (runtime) => {
        const check = await invoke<RuntimeCheck>("check_runtime", { runtime });
        return [runtime, check] as const;
      }),
    );
    setRuntimeChecks(Object.fromEntries(entries));
  }

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
      return null;
    });
  }

  async function mutate(command: string, args: Record<string, unknown> = {}) {
    try {
      await invoke(command, args);
      await refresh();
    } catch (err) {
      const message = errorMessage(err, `${command} failed`);
      setAppError(message);
      console.error(err);
      throw err;
    }
  }

  useEffect(() => {
    refresh().catch((err) => {
      setAppError(errorMessage(err, "Failed to load LocalSlock state"));
      console.error(err);
    });
    refreshRuntimeChecks().catch((err) => {
      setAppError(errorMessage(err, "Failed to check local runtimes"));
      console.error(err);
    });
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      refresh().catch((err) => {
        setAppError(errorMessage(err, "Failed to refresh LocalSlock state"));
        console.error(err);
      });
    }, 1500);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!appError) return;
    const timer = window.setTimeout(() => setAppError(null), 6500);
    return () => window.clearTimeout(timer);
  }, [appError]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const modifier = event.metaKey || event.ctrlKey;
      const channels = data?.channels ?? [];

      if (modifier && event.key.toLowerCase() === "k") {
        event.preventDefault();
        document.getElementById("local-slock-search")?.focus();
        return;
      }

      if (modifier && (event.key === "[" || event.key === "]") && channels.length > 0) {
        event.preventDefault();
        const currentIndex = Math.max(0, channels.findIndex((item) => item.id === activeChannelId));
        const offset = event.key === "]" ? 1 : -1;
        const nextIndex = (currentIndex + offset + channels.length) % channels.length;
        selectChannel(channels[nextIndex].id);
        return;
      }

      const modalOpen =
        showCreateChannelModal ||
        showChannelSettingsModal ||
        showChannelAgentsModal ||
        showCreateAgentModal ||
        Boolean(editingMessage) ||
        Boolean(editingAgentId);
      if (event.key === "Escape" && !modalOpen && !isTextInput(event.target)) {
        if (selectedAgentId) {
          setSelectedAgentId(null);
        } else if (showThread) {
          setShowThread(false);
        }
      }
    }

    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [
    activeChannelId,
    data?.channels,
    editingAgentId,
    editingMessage,
    selectedAgentId,
    showChannelAgentsModal,
    showChannelSettingsModal,
    showCreateAgentModal,
    showCreateChannelModal,
    showThread,
  ]);

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
    if (channel.kind === "dm") return [];
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

  const activeChannelMessageCount = useMemo(() => {
    if (!data || !activeChannelId) return 0;
    return data.messages.filter((message) => message.channel_id === activeChannelId).length;
  }, [data?.messages, activeChannelId]);

  const activeRunFor = useCallback((agentId: string) => {
    return data?.agent_runs.find((run) => run.agent_id === agentId && ACTIVE_RUN_STATUSES.has(run.status)) ?? null;
  }, [data?.agent_runs]);

  const selectedAgent = useMemo(() => {
    if (!data || !selectedAgentId) return null;
    return data.agents.find((agent) => agent.id === selectedAgentId) ?? null;
  }, [data, selectedAgentId]);

  const selectedAgentRun = useMemo(() => {
    if (!selectedAgent) return null;
    return activeRunFor(selectedAgent.id);
  }, [selectedAgent, activeRunFor]);

  const selectedAgentActivities = useMemo(() => {
    if (!data || !selectedAgent) return [];
    return data.agent_activities
      .filter((activity) => activity.agent_id === selectedAgent.id || activity.agent_handle === selectedAgent.handle)
      .slice(0, 80);
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
      .filter((item) => {
        const dmAgent = item.kind === "dm" ? data.agents.find((agent) => agent.id === item.dm_agent_id) : null;
        return `${item.name} ${item.description} ${dmAgent?.handle ?? ""} ${dmAgent?.display_name ?? ""}`
          .toLowerCase()
          .includes(query);
      })
      .map((item) => {
        const dmAgent = item.kind === "dm" ? data.agents.find((agent) => agent.id === item.dm_agent_id) : null;
        return {
          id: item.id,
          kind: item.kind === "dm" ? "dm" : "channel",
          title: item.kind === "dm" ? `@${dmAgent?.handle ?? "agent"}` : `#${item.name}`,
          detail: item.kind === "dm" ? dmAgent?.display_name ?? "direct message" : item.description || "channel",
          channelId: item.id,
          threadId: null,
          agentId: null,
        };
      });

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
    if (channel?.kind !== "dm") return;
    setActiveTab("chat");
    setShowChannelSettingsModal(false);
    setShowChannelAgentsModal(false);
  }, [channel?.id, channel?.kind]);

  useEffect(() => {
    if (!activeChannelId) return;
    invoke("mark_channel_read", { channelId: activeChannelId }).catch((err) => console.error(err));
  }, [activeChannelId, activeChannelMessageCount]);

  async function createChannel() {
    const name = newChannel.trim().replace(/^#/, "");
    if (!name) return;
    await mutate("create_channel", { name });
    setNewChannel("");
    setShowCreateChannelModal(false);
  }

  async function saveChannel() {
    if (!channel || !channelNameDraft.trim()) return;
    if (channel.kind === "dm") {
      setAppError("Direct message settings are managed by the agent profile");
      setShowChannelSettingsModal(false);
      return;
    }
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
    const nextChannel = data?.channels.find((item) => item.id === channelId) ?? null;
    setActiveChannelId(channelId);
    setDraft("");
    setReplyDraft("");
    setTaskDraft("");
    if (nextChannel?.kind === "dm") {
      setActiveTab("chat");
    }
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
    if (channel.kind === "dm") {
      setAppError("Direct message membership is fixed");
      return;
    }
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
      if (channel.kind !== "dm") {
        await invoke("set_channel_agent_membership", {
          channelId: channel.id,
          agentId,
          member: true,
        });
      }
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
      asTask: channel.kind === "dm" ? false : asTask,
    });
    setDraft("");
  }

  async function createTaskFromBoard() {
    if (!channel || !taskDraft.trim()) return;
    if (channel.kind === "dm") {
      setAppError("Direct messages do not support tasks");
      return;
    }
    await mutate("send_message", {
      channelId: channel.id,
      threadRootId: null,
      body: taskDraft.trim(),
      asTask: true,
    });
    setTaskDraft("");
  }

  async function openDmWithAgent(agent: Agent) {
    try {
      const channelId = await invoke<string>("open_dm_with_agent", { agentId: agent.id });
      await refresh();
      setActiveChannelId(channelId);
      setActiveThreadId(null);
      setActiveTab("chat");
      setDraft("");
      setReplyDraft("");
      setTaskDraft("");
      setSelectedAgentId(null);
    } catch (err) {
      const message = errorMessage(err, "Failed to open direct message");
      setAppError(message);
      console.error(err);
    }
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

  function editMessage(message: Message) {
    setEditingMessage(message);
    setMessageEditDraft(message.body);
  }

  async function saveMessageEdit() {
    if (!editingMessage) return;
    const body = messageEditDraft.trim();
    if (!body || body === editingMessage.body) {
      setEditingMessage(null);
      setMessageEditDraft("");
      return;
    }
    await mutate("update_message", { messageId: editingMessage.id, body });
    setEditingMessage(null);
    setMessageEditDraft("");
  }

  async function deleteMessage(message: Message) {
    const isThreadRoot = !message.thread_root_id;
    const warning = isThreadRoot
      ? "Delete this message and all thread replies/tasks attached to it?"
      : "Delete this reply?";
    if (!window.confirm(warning)) return;
    await mutate("delete_message", { messageId: message.id });
    if (isThreadRoot && activeThreadId === message.id) {
      setActiveThreadId(null);
    }
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
    if (result.channelId) selectChannel(result.channelId);
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

  async function startAgent(agent: Agent) {
    await mutate("start_agent", { agentId: agent.id });
  }

  async function stopAgent(run: AgentRun) {
    if (!window.confirm(`Stop @${run.agent_handle}? Current work will be interrupted.`)) return;
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
    <main className={`app theme-liquid ${selectedAgent || showThread ? "" : "thread-hidden"}`}>
      <Sidebar
        data={data}
        channel={channel}
        threadRootMessages={threadedRootMessages}
        allRootCount={rootMessages.length}
        threadCount={threadedRootMessages.length}
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
        openDmWithAgent={openDmWithAgent}
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
        editMessage={editMessage}
        deleteMessage={deleteMessage}
      />

      {selectedAgent ? (
        <AgentDetailDrawer
          agent={selectedAgent}
          activeRun={selectedAgentRun}
          phase={selectedAgentPhase}
          activities={selectedAgentActivities}
          workItems={selectedAgentWorkItems}
          onClose={() => setSelectedAgentId(null)}
          onDelete={deleteAgent}
          onStart={startAgent}
          onStop={stopAgent}
          onEdit={(agent) => {
            startEditAgent(agent);
            setSelectedAgentId(null);
          }}
          onOpenDm={openDmWithAgent}
          onOpenWorkItem={(item) => {
            openWorkItem(item);
            setSelectedAgentId(null);
          }}
        />
      ) : showThread && (
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
          editMessage={editMessage}
          deleteMessage={deleteMessage}
          setReplyDraft={setReplyDraft}
          sendReply={sendReply}
        />
      )}

      {appError && (
        <div className="app-toast error" role="alert">
          <span>{appError}</span>
          <button onClick={() => setAppError(null)} aria-label="Dismiss error">Dismiss</button>
        </div>
      )}

      <CreateChannelModal
        open={showCreateChannelModal}
        channelName={newChannel}
        onChange={setNewChannel}
        onCancel={() => setShowCreateChannelModal(false)}
        onSubmit={createChannel}
      />

      <ChannelSettingsModal
        open={showChannelSettingsModal}
        channel={channel}
        agents={data.agents}
        channelMemberIds={channelMemberIds}
        nameDraft={channelNameDraft}
        descriptionDraft={channelDescriptionDraft}
        onNameChange={setChannelNameDraft}
        onDescriptionChange={setChannelDescriptionDraft}
        onSetMember={setChannelMember}
        onDelete={deleteChannel}
        onCancel={() => setShowChannelSettingsModal(false)}
        onSave={saveChannel}
      />

      <ChannelAgentsModal
        open={showChannelAgentsModal}
        channel={channel}
        agents={data.agents}
        channelMemberIds={channelMemberIds}
        onSetMember={setChannelMember}
        onCreateAgent={() => {
          setShowChannelAgentsModal(false);
          setShowCreateAgentModal(true);
        }}
        onClose={() => setShowChannelAgentsModal(false)}
      />

      <AgentFormModal
        open={showCreateAgentModal}
        title="Add Agent"
        form={agentDraft}
        runtimeChecks={runtimeChecks}
        submitLabel="Add agent"
        onChange={setAgentDraft}
        onRuntimeChange={updateDraftRuntime}
        onCancel={() => setShowCreateAgentModal(false)}
        onSubmit={createAgent}
      />

      <AgentFormModal
        open={Boolean(editingAgentId)}
        title="Edit Agent"
        form={agentEdit}
        runtimeChecks={runtimeChecks}
        submitLabel="Save"
        showNotes
        onChange={setAgentEdit}
        onRuntimeChange={updateEditRuntime}
        onCancel={cancelEditAgent}
        onSubmit={saveAgent}
      />

      <Modal
        open={Boolean(editingMessage)}
        title="Edit Message"
        onClose={() => {
          setEditingMessage(null);
          setMessageEditDraft("");
        }}
        width={640}
      >
        <div className="modal-form">
          <label>
            <span>Message body</span>
            <textarea
              autoFocus
              value={messageEditDraft}
              onChange={(event) => setMessageEditDraft(event.target.value)}
              rows={7}
            />
          </label>
          <div className="modal-actions">
            <button
              onClick={() => {
                setEditingMessage(null);
                setMessageEditDraft("");
              }}
            >
              Cancel
            </button>
            <button className="primary" disabled={!messageEditDraft.trim()} onClick={saveMessageEdit}>
              Save
            </button>
          </div>
        </div>
      </Modal>

    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
