import { useEffect, useMemo, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { Conversation } from "./components/Conversation";
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
} from "./types";
import { buildPresetCommand, firstLines, formatTime } from "./ui-utils";
import "./styles.css";

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
  const [dispatchAgentId, setDispatchAgentId] = useState("");
  const [dispatchContext, setDispatchContext] = useState("");
  const [workAgentFilter, setWorkAgentFilter] = useState("");
  const [workStatusFilter, setWorkStatusFilter] = useState("active");

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
    setDispatchAgentId((prev) => {
      if (data?.agents.some((agent) => agent.id === prev)) return prev;
      return data?.agents[0]?.id || "";
    });
  }, [data?.agents]);

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

  const channelMemberIds = useMemo(() => {
    if (!data || !channel) return new Set<string>();
    return new Set(data.channel_members.filter((member) => member.channel_id === channel.id).map((member) => member.agent_id));
  }, [data, channel]);

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

  function selectChannel(channelId: string) {
    setActiveChannelId(channelId);
    const first = data?.messages.find((m) => m.channel_id === channelId && !m.thread_root_id);
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

  function cancelEditAgent() {
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

  function openWorkItem(item: AgentWorkItem) {
    if (item.channel_id) setActiveChannelId(item.channel_id);
    if (item.thread_root_id) {
      setActiveThreadId(item.thread_root_id);
      setActiveTab("chat");
    }
    const agent = data?.agents.find((candidate) => candidate.id === item.agent_id);
    if (agent) startEditAgent(agent);
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

  function buildDispatchContext() {
    const parts = [];
    if (channel) parts.push(`Channel: #${channel.name}`);
    if (activeTask) {
      parts.push(`Task #${activeTask.number}: ${activeTask.title}`);
      parts.push(`Task status: ${activeTask.status}`);
    }
    if (activeRoot) {
      parts.push(`Thread root by ${activeRoot.sender_name} at ${formatTime(activeRoot.created_at)}:`);
      parts.push(activeRoot.body);
    }
    if (replies.length > 0) {
      parts.push("Recent replies:");
      replies.slice(-8).forEach((reply) => {
        parts.push(`- ${reply.sender_name} at ${formatTime(reply.created_at)}: ${reply.body}`);
      });
    }
    if (dispatchContext.trim()) {
      parts.push("Human instruction:");
      parts.push(dispatchContext.trim());
    }
    return parts.join("\n\n");
  }

  async function dispatchCurrentContext() {
    if (!channel || !dispatchAgentId) return;
    const title = activeTask?.title || (activeRoot ? firstLines(activeRoot.body, 1) : `Work in #${channel.name}`);
    await mutate("dispatch_agent_work", {
      agentId: dispatchAgentId,
      channelId: channel.id,
      threadRootId: activeRoot?.id ?? null,
      taskId: activeTask?.id ?? null,
      title,
      context: buildDispatchContext(),
    });
    setDispatchContext("");
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

  const draftPresetCommand = buildPresetCommand(agentDraft);
  const editPresetCommand = buildPresetCommand(agentEdit);

  if (!data) {
    return <div className="boot">Opening LocalSlock...</div>;
  }

  return (
    <main className="app theme-liquid">
      <Sidebar
        data={data}
        channel={channel}
        rootMessages={rootMessages}
        followedThreads={followedThreads}
        searchQuery={searchQuery}
        searchResults={searchResults}
        newChannel={newChannel}
        channelNameDraft={channelNameDraft}
        channelDescriptionDraft={channelDescriptionDraft}
        channelMemberIds={channelMemberIds}
        agentDraft={agentDraft}
        editingAgentId={editingAgentId}
        agentEdit={agentEdit}
        draftPresetCommand={draftPresetCommand}
        editPresetCommand={editPresetCommand}
        setSearchQuery={setSearchQuery}
        openSearchResult={openSearchResult}
        setNewChannel={setNewChannel}
        createChannel={createChannel}
        selectChannel={selectChannel}
        setChannelNameDraft={setChannelNameDraft}
        setChannelDescriptionDraft={setChannelDescriptionDraft}
        saveChannel={saveChannel}
        deleteChannel={deleteChannel}
        setChannelMember={setChannelMember}
        setAgentDraft={setAgentDraft}
        updateDraftRuntime={updateDraftRuntime}
        applyDraftPreset={applyDraftPreset}
        createAgent={createAgent}
        activeRunFor={activeRunFor}
        startAgent={startAgent}
        stopAgent={stopAgent}
        deleteAgent={deleteAgent}
        startEditAgent={startEditAgent}
        setAgentEdit={setAgentEdit}
        updateEditRuntime={updateEditRuntime}
        applyEditPreset={applyEditPreset}
        saveAgent={saveAgent}
        cancelEditAgent={cancelEditAgent}
      />

      <Conversation
        channel={channel}
        agents={data.agents}
        activeTab={activeTab}
        activeRoot={activeRoot}
        rootMessages={rootMessages}
        visibleTasks={visibleTasks}
        draft={draft}
        taskDraft={taskDraft}
        taskTitleDrafts={taskTitleDrafts}
        setActiveTab={setActiveTab}
        setActiveThreadId={setActiveThreadId}
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

      <ThreadPanel
        data={data}
        channel={channel}
        activeRoot={activeRoot}
        activeTask={activeTask}
        replies={replies}
        dispatchAgentId={dispatchAgentId}
        dispatchContext={dispatchContext}
        workAgentFilter={workAgentFilter}
        workStatusFilter={workStatusFilter}
        visibleWorkItems={visibleWorkItems}
        queuedWorkItemCount={queuedWorkItemCount}
        taskTitleDrafts={taskTitleDrafts}
        replyDraft={replyDraft}
        setDispatchAgentId={setDispatchAgentId}
        setDispatchContext={setDispatchContext}
        dispatchCurrentContext={dispatchCurrentContext}
        setWorkAgentFilter={setWorkAgentFilter}
        setWorkStatusFilter={setWorkStatusFilter}
        openWorkItem={openWorkItem}
        cancelWorkItem={cancelWorkItem}
        retryWorkItem={retryWorkItem}
        installSupervisorService={installSupervisorService}
        uninstallSupervisorService={uninstallSupervisorService}
        toggleThreadFollow={toggleThreadFollow}
        setActiveThreadId={setActiveThreadId}
        setTaskTitleDraft={setTaskTitleDraft}
        saveTaskTitle={saveTaskTitle}
        claimTask={claimTask}
        updateTaskStatus={updateTaskStatus}
        setReplyDraft={setReplyDraft}
        sendReply={sendReply}
      />
    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
