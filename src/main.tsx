import { type CSSProperties, type PointerEvent as ReactPointerEvent, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { createRoot } from "react-dom/client";
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import { AgentDetailDrawer } from "./components/AgentDetailDrawer";
import { AgentFormModal } from "./components/AgentFormModal";
import { ChannelAgentsModal } from "./components/ChannelAgentsModal";
import { ChannelSettingsModal } from "./components/ChannelSettingsModal";
import { Conversation } from "./components/Conversation";
import { CreateChannelModal } from "./components/CreateChannelModal";
import { SearchModal } from "./components/SearchModal";
import { Sidebar } from "./components/Sidebar";
import { ThreadBrowserModal } from "./components/ThreadBrowserModal";
import { ThreadPanel } from "./components/ThreadPanel";
import {
  ACTIVE_RUN_STATUSES,
  Agent,
  AgentActivity,
  AgentForm,
  AgentRun,
  AgentWorkItem,
  Bootstrap,
  EMPTY_AGENT_FORM,
  Message,
  RUNTIME_PRESETS,
  RuntimeCheck,
  SearchResult,
  SearchScope,
  SearchTimeRange,
  Task,
} from "./types";
import { buildPresetCommand, firstLines, formatTime } from "./ui-utils";
import "./styles.css";

const ACTIVITY_PHASE_LABELS: Record<string, string> = {
  thinking: "Thinking",
  runtime: "Runtime",
  work: "Work",
  profile: "Profile",
  acting: "Acting",
  tools: "Using tools",
  error: "Error",
  event: "Acting",
  message: "Acting",
  task: "Acting",
  event_error: "Error",
  run_error: "Error",
};

const DEFAULT_THREAD_PANEL_WIDTH = 420;
const MIN_THREAD_PANEL_WIDTH = 320;
const DEFAULT_SIDEBAR_WIDTH = 292;
const MIN_SIDEBAR_WIDTH = 240;
const MAX_SIDEBAR_WIDTH = 460;
const MIN_CONVERSATION_WIDTH = 420;
const UI_REFRESH_EVENT = "localslock://refresh";
const UI_REFRESH_DEBOUNCE_MS = 80;

type UiBackendEvent =
  | { type: "refresh"; reason?: string }
  | { type: "message_upsert"; reason?: string; message: Message }
  | { type: "message_delta"; reason?: string; message_id: string; append: string; delivery_state: Message["delivery_state"] }
  | { type: "activity_upsert"; reason?: string; activity: AgentActivity }
  | { type: "agent_run_upsert"; reason?: string; run: Omit<AgentRun, "log"> & { log?: string } }
  | { type: "work_item_upsert"; reason?: string; work_item: Omit<AgentWorkItem, "context"> & { context?: string } };

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

function maxThreadPanelWidth() {
  return Math.max(MIN_THREAD_PANEL_WIDTH, Math.floor(window.innerWidth * 0.66));
}

function matchesSearchTime(value: string | null, range: SearchTimeRange) {
  if (range === "any" || !value) return true;
  const timestamp = new Date(value).getTime();
  if (Number.isNaN(timestamp)) return true;
  const now = Date.now();
  if (range === "today") {
    const start = new Date();
    start.setHours(0, 0, 0, 0);
    return timestamp >= start.getTime();
  }
  const days = range === "7d" ? 7 : 30;
  return now - timestamp <= days * 24 * 60 * 60 * 1000;
}

function searchScopeAllows(scope: SearchScope, kind: SearchScope) {
  return scope === "all" || scope === kind;
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
  const [searchScope, setSearchScope] = useState<SearchScope>("all");
  const [searchTimeRange, setSearchTimeRange] = useState<SearchTimeRange>("any");
  const [newChannel, setNewChannel] = useState("");
  const [channelNameDraft, setChannelNameDraft] = useState("");
  const [channelDescriptionDraft, setChannelDescriptionDraft] = useState("");
  const [agentDraft, setAgentDraft] = useState<AgentForm>(EMPTY_AGENT_FORM);
  const [editingAgentId, setEditingAgentId] = useState<string | null>(null);
  const [agentEdit, setAgentEdit] = useState<AgentForm>(EMPTY_AGENT_FORM);
  const [showThread, setShowThread] = useState(true);
  const [showCreateChannelModal, setShowCreateChannelModal] = useState(false);
  const [showChannelSettingsModal, setShowChannelSettingsModal] = useState(false);
  const [showChannelAgentsModal, setShowChannelAgentsModal] = useState(false);
  const [showCreateAgentModal, setShowCreateAgentModal] = useState(false);
  const [showSearchModal, setShowSearchModal] = useState(false);
  const [showThreadBrowserModal, setShowThreadBrowserModal] = useState(false);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [appError, setAppError] = useState<string | null>(null);
  const [runtimeChecks, setRuntimeChecks] = useState<Record<string, RuntimeCheck>>({});
  const [threadPanelWidth, setThreadPanelWidth] = useState(() => {
    const stored = window.localStorage.getItem("localslock.threadPanelWidth");
    const value = stored ? Number(stored) : DEFAULT_THREAD_PANEL_WIDTH;
    return Number.isFinite(value)
      ? Math.min(maxThreadPanelWidth(), Math.max(MIN_THREAD_PANEL_WIDTH, value))
      : DEFAULT_THREAD_PANEL_WIDTH;
  });
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const stored = window.localStorage.getItem("localslock.sidebarWidth");
    const value = stored ? Number(stored) : DEFAULT_SIDEBAR_WIDTH;
    return Number.isFinite(value)
      ? Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, value))
      : DEFAULT_SIDEBAR_WIDTH;
  });
  const [channelAlertIds, setChannelAlertIds] = useState<Set<string>>(() => new Set());
  const [threadUnreadCounts, setThreadUnreadCounts] = useState<Record<string, number>>({});
  const [locallyUnfollowedThreadIds, setLocallyUnfollowedThreadIds] = useState<Set<string>>(() => new Set());
  const knownMessageIdsRef = useRef<Set<string> | null>(null);
  const refreshTimerRef = useRef<number | null>(null);
  const refreshInFlightRef = useRef(false);
  const refreshQueuedRef = useRef(false);

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

  function refreshWithError(fallback: string) {
    if (refreshInFlightRef.current) {
      refreshQueuedRef.current = true;
      return;
    }
    refreshInFlightRef.current = true;
    refresh()
      .catch((err) => {
        setAppError(errorMessage(err, fallback));
        console.error(err);
      })
      .finally(() => {
        refreshInFlightRef.current = false;
        if (refreshQueuedRef.current) {
          refreshQueuedRef.current = false;
          requestRefresh(fallback);
        }
      });
  }

  function requestRefresh(fallback = "Failed to refresh LocalSlock state") {
    if (refreshTimerRef.current !== null) return;
    refreshTimerRef.current = window.setTimeout(() => {
      refreshTimerRef.current = null;
      refreshWithError(fallback);
    }, UI_REFRESH_DEBOUNCE_MS);
  }

  function applyMessageUpsert(message: Message) {
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after message update");
        return current;
      }
      const existingIndex = current.messages.findIndex((item) => item.id === message.id);
      const messages = existingIndex >= 0
        ? current.messages.map((item) => item.id === message.id ? message : item)
        : [...current.messages, message];
      messages.sort((left, right) => new Date(left.created_at).getTime() - new Date(right.created_at).getTime());
      return { ...current, messages };
    });
  }

  function applyMessageDelta(messageId: string, append: string, deliveryState: Message["delivery_state"]) {
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after message delta");
        return current;
      }
      const existingIndex = current.messages.findIndex((item) => item.id === messageId);
      if (existingIndex < 0) {
        requestRefresh("Failed to refresh LocalSlock state after message delta");
        return current;
      }
      const messages = current.messages.map((item) => item.id === messageId
        ? { ...item, body: `${item.body}${append}`, delivery_state: deliveryState }
        : item);
      return { ...current, messages };
    });
  }

  function applyActivityUpsert(activity: AgentActivity) {
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after activity update");
        return current;
      }
      const existingIndex = current.agent_activities.findIndex((item) => item.id === activity.id);
      const agent_activities = existingIndex >= 0
        ? current.agent_activities.map((item) => item.id === activity.id ? activity : item)
        : [activity, ...current.agent_activities];
      agent_activities.sort((left, right) => new Date(right.created_at).getTime() - new Date(left.created_at).getTime());
      return { ...current, agent_activities: agent_activities.slice(0, 80) };
    });
  }

  function applyAgentRunUpsert(patch: Omit<AgentRun, "log"> & { log?: string }) {
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after run update");
        return current;
      }
      const existing = current.agent_runs.find((item) => item.id === patch.id);
      const run: AgentRun = {
        ...patch,
        log: patch.log ?? existing?.log ?? "",
      };
      const agent_runs = existing
        ? current.agent_runs.map((item) => item.id === patch.id ? { ...item, ...run } : item)
        : [run, ...current.agent_runs];
      agent_runs.sort((left, right) => new Date(right.started_at).getTime() - new Date(left.started_at).getTime());
      return { ...current, agent_runs: agent_runs.slice(0, 30) };
    });
  }

  function applyWorkItemUpsert(patch: Omit<AgentWorkItem, "context"> & { context?: string }) {
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after work item update");
        return current;
      }
      const existing = current.agent_work_items.find((item) => item.id === patch.id);
      const workItem: AgentWorkItem = {
        ...patch,
        context: patch.context ?? existing?.context ?? "",
      };
      const agent_work_items = existing
        ? current.agent_work_items.map((item) => item.id === patch.id ? { ...item, ...workItem } : item)
        : [workItem, ...current.agent_work_items];
      agent_work_items.sort((left, right) => new Date(right.created_at).getTime() - new Date(left.created_at).getTime());
      return { ...current, agent_work_items: agent_work_items.slice(0, 80) };
    });
  }

  function handleBackendEvent(payload: unknown) {
    if (typeof payload !== "string") {
      requestRefresh("Failed to refresh LocalSlock state after backend update");
      return;
    }
    let parsed: UiBackendEvent | null = null;
    try {
      parsed = JSON.parse(payload) as UiBackendEvent;
    } catch {
      requestRefresh("Failed to refresh LocalSlock state after backend update");
      return;
    }
    if (parsed.type === "message_upsert") {
      applyMessageUpsert(parsed.message);
      return;
    }
    if (parsed.type === "message_delta") {
      applyMessageDelta(parsed.message_id, parsed.append, parsed.delivery_state);
      return;
    }
    if (parsed.type === "activity_upsert") {
      applyActivityUpsert(parsed.activity);
      return;
    }
    if (parsed.type === "agent_run_upsert") {
      applyAgentRunUpsert(parsed.run);
      return;
    }
    if (parsed.type === "work_item_upsert") {
      applyWorkItemUpsert(parsed.work_item);
      return;
    }
    requestRefresh("Failed to refresh LocalSlock state after backend update");
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
    let unlisten: UnlistenFn | null = null;
    listen<string>(UI_REFRESH_EVENT, (event) => {
      handleBackendEvent(event.payload);
    })
      .then((handler) => {
        unlisten = handler;
      })
      .catch((err) => {
        setAppError(errorMessage(err, "Failed to subscribe to LocalSlock updates"));
        console.error(err);
      });
    return () => {
      if (refreshTimerRef.current !== null) {
        window.clearTimeout(refreshTimerRef.current);
      }
      unlisten?.();
    };
  }, []);

  useEffect(() => {
    const timer = window.setInterval(() => {
      requestRefresh("Failed to refresh LocalSlock state");
    }, 5000);
    return () => window.clearInterval(timer);
  }, []);

  useEffect(() => {
    if (!data) return;
    if (!knownMessageIdsRef.current) {
      knownMessageIdsRef.current = new Set(data.messages.map((message) => message.id));
      return;
    }

    const known = knownMessageIdsRef.current;
    const newMessages = data.messages.filter((message) => !known.has(message.id));
    if (newMessages.length === 0) return;
    newMessages.forEach((message) => known.add(message.id));

    setChannelAlertIds((current) => {
      let next: Set<string> | null = null;
      for (const message of newMessages) {
        if (message.channel_id === activeChannelId) continue;
        next ??= new Set(current);
        next.add(message.channel_id);
      }
      return next ?? current;
    });

    setThreadUnreadCounts((current) => {
      let next: Record<string, number> | null = null;
      for (const message of newMessages) {
        if (!message.thread_root_id || message.thread_root_id === activeThreadId) continue;
        next ??= { ...current };
        next[message.thread_root_id] = (next[message.thread_root_id] ?? 0) + 1;
      }
      return next ?? current;
    });
  }, [activeChannelId, activeThreadId, data]);

  useEffect(() => {
    if (!appError) return;
    const timer = window.setTimeout(() => setAppError(null), 6500);
    return () => window.clearTimeout(timer);
  }, [appError]);

  useEffect(() => {
    if (!activeChannelId) return;
    setChannelAlertIds((current) => {
      if (!current.has(activeChannelId)) return current;
      const next = new Set(current);
      next.delete(activeChannelId);
      return next;
    });
  }, [activeChannelId]);

  useEffect(() => {
    if (!activeThreadId) return;
    setThreadUnreadCounts((current) => {
      if (!current[activeThreadId]) return current;
      const next = { ...current };
      delete next[activeThreadId];
      return next;
    });
  }, [activeThreadId]);

  useEffect(() => {
    window.localStorage.setItem("localslock.threadPanelWidth", String(threadPanelWidth));
  }, [threadPanelWidth]);

  useEffect(() => {
    window.localStorage.setItem("localslock.sidebarWidth", String(sidebarWidth));
  }, [sidebarWidth]);

  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const modifier = event.metaKey || event.ctrlKey;
      const channels = data?.channels ?? [];

      if (modifier && event.key.toLowerCase() === "k") {
        event.preventDefault();
        setShowSearchModal(true);
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
        showSearchModal ||
        showThreadBrowserModal ||
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
    selectedAgentId,
    showChannelAgentsModal,
    showChannelSettingsModal,
    showCreateAgentModal,
    showCreateChannelModal,
    showSearchModal,
    showThreadBrowserModal,
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

  const threadReplyCounts = useMemo(() => {
    if (!data) return {};
    return data.messages.reduce<Record<string, number>>((counts, message) => {
      if (!message.thread_root_id) return counts;
      counts[message.thread_root_id] = (counts[message.thread_root_id] ?? 0) + 1;
      return counts;
    }, {});
  }, [data?.messages]);

  const allThreadRootMessages = useMemo(() => {
    if (!data) return [];
    const latestByRoot = new Map<string, number>();
    for (const message of data.messages) {
      if (!message.thread_root_id) continue;
      const timestamp = new Date(message.created_at).getTime();
      latestByRoot.set(message.thread_root_id, Math.max(latestByRoot.get(message.thread_root_id) ?? 0, timestamp));
    }
    return data.messages
      .filter((message) =>
        !message.thread_root_id &&
        latestByRoot.has(message.id) &&
        (message.thread_followed || (threadUnreadCounts[message.id] ?? 0) > 0) &&
        !locallyUnfollowedThreadIds.has(message.id))
      .sort((left, right) => (latestByRoot.get(right.id) ?? 0) - (latestByRoot.get(left.id) ?? 0));
  }, [data?.messages, locallyUnfollowedThreadIds, threadUnreadCounts]);

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
        kind: selectedAgentLiveActivity?.phase ?? selectedAgentLiveActivity?.kind ?? "run",
        label: selectedAgentLiveActivity ? phaseForActivity(selectedAgentLiveActivity.phase || selectedAgentLiveActivity.kind) : "Running",
        detail: selectedAgentLiveActivity?.summary || selectedAgentLiveActivity?.detail || "Waiting for observable output from the agent.",
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
    const channelById = new Map(data.channels.map((item) => [item.id, item]));
    const agentById = new Map(data.agents.map((item) => [item.id, item]));
    const channelLabel = (channelId: string | null) => {
      if (!channelId) return "No channel";
      const target = channelById.get(channelId);
      if (!target) return "Unknown channel";
      if (target.kind === "dm") {
        const agent = target.dm_agent_id ? agentById.get(target.dm_agent_id) : null;
        return agent ? `DM with @${agent.handle}` : "Direct message";
      }
      return `#${target.name}`;
    };
    const includes = (value: string) => value.toLowerCase().includes(query);
    const results: SearchResult[] = [];

    if (searchScopeAllows(searchScope, "channels")) {
      results.push(...data.channels
        .filter((item) => {
        const dmAgent = item.kind === "dm" ? data.agents.find((agent) => agent.id === item.dm_agent_id) : null;
          return includes(`${item.name} ${item.description} ${dmAgent?.handle ?? ""} ${dmAgent?.display_name ?? ""}`);
        })
        .map((item) => {
        const dmAgent = item.kind === "dm" ? data.agents.find((agent) => agent.id === item.dm_agent_id) : null;
        return {
          id: item.id,
          kind: item.kind === "dm" ? "dm" : "channel",
          title: item.kind === "dm" ? `@${dmAgent?.handle ?? "agent"}` : `#${item.name}`,
          detail: item.kind === "dm" ? dmAgent?.display_name ?? "direct message" : item.description || "channel",
          excerpt: item.kind === "dm" ? dmAgent?.description ?? "" : item.description,
          createdAt: null,
          channelId: item.id,
          threadId: null,
          agentId: null,
        };
        }).slice(0, 10));
    }

    if (searchScopeAllows(searchScope, "tasks")) {
      results.push(...data.tasks
        .filter((item) =>
          matchesSearchTime(item.updated_at, searchTimeRange) &&
          includes(`${item.title} ${item.status} ${item.channel_name} ${item.assignee_name ?? ""}`))
        .map((item) => ({
        id: item.id,
        kind: "task",
        title: `#${item.number} ${item.title}`,
        detail: `${item.channel_name} · ${item.status.replace("_", " ")}`,
        excerpt: item.assignee_name ? `Assigned to ${item.assignee_name}` : "Unassigned",
        createdAt: item.updated_at,
        channelId: item.channel_id,
        threadId: item.message_id,
        agentId: null,
      })).slice(0, 12));
    }

    if (searchScopeAllows(searchScope, "messages")) {
      results.push(...data.messages
        .filter((item) =>
          matchesSearchTime(item.created_at, searchTimeRange) &&
          includes(`${item.sender_name} ${item.body} ${channelLabel(item.channel_id)}`))
        .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
        .map((item) => ({
        id: item.id,
        kind: item.thread_root_id ? "reply" : "message",
        title: item.sender_name,
        detail: `${channelLabel(item.channel_id)} · ${item.thread_root_id ? "thread reply" : "message"} · ${formatTime(item.created_at)}`,
        excerpt: firstLines(item.body, 2),
        createdAt: item.created_at,
        channelId: item.channel_id,
        threadId: item.thread_root_id ?? item.id,
        agentId: null,
      })).slice(0, 40));
    }

    if (searchScopeAllows(searchScope, "agents")) {
      results.push(...data.agents
        .filter((item) => includes(`${item.handle} ${item.display_name} ${item.runtime} ${item.model} ${item.description}`))
        .map((item) => ({
        id: item.id,
        kind: "agent",
        title: `@${item.handle}`,
        detail: `${item.display_name} · ${item.runtime} · ${item.status}`,
        excerpt: item.description,
        createdAt: null,
        channelId: null,
        threadId: null,
        agentId: item.id,
      })).slice(0, 10));
    }

    if (searchScopeAllows(searchScope, "activity")) {
      results.push(...data.agent_activities
        .filter((item) =>
          matchesSearchTime(item.created_at, searchTimeRange) &&
          includes(`${item.agent_handle} ${item.kind} ${item.title} ${item.detail}`))
        .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
        .map((item) => ({
        id: item.id,
        kind: "activity",
        title: item.title,
        detail: `${item.agent_handle || "unknown"} · ${formatTime(item.created_at)}`,
        excerpt: item.detail,
        createdAt: item.created_at,
        channelId: null,
        threadId: null,
        agentId: item.agent_id,
      })).slice(0, 16));

      results.push(...data.agent_work_items
        .filter((item) =>
          matchesSearchTime(item.updated_at, searchTimeRange) &&
          includes(`${item.agent_handle} ${item.status} ${item.title} ${item.context}`))
        .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
        .map((item) => ({
        id: item.id,
        kind: "work",
        title: item.title,
        detail: `${item.agent_handle} · ${item.status} · ${channelLabel(item.channel_id)}`,
        excerpt: firstLines(item.context, 2),
        createdAt: item.updated_at,
        channelId: item.channel_id,
        threadId: item.thread_root_id,
        agentId: item.agent_id,
      })).slice(0, 16));
    }

    return results.slice(0, 80);
  }, [data, searchQuery, searchScope, searchTimeRange]);

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
    openThread(first?.id ?? null);
  }

  function openThread(threadId: string | null) {
    setActiveThreadId(threadId);
    if (!threadId) return;
    setThreadUnreadCounts((current) => {
      if (!current[threadId]) return current;
      const next = { ...current };
      delete next[threadId];
      return next;
    });
  }

  function addOptimisticOwnerMessage(channelId: string, threadRootId: string | null, body: string, asTask: boolean) {
    const id = `local-${crypto.randomUUID()}`;
    const optimisticMessage: Message = {
      id,
      channel_id: channelId,
      thread_root_id: threadRootId,
      sender_name: "Dylan",
      sender_role: "owner",
      body,
      is_task: asTask,
      thread_followed: true,
      delivery_state: "complete",
      stream_key: "",
      task_number: null,
      task_status: null,
      created_at: new Date().toISOString(),
    };
    knownMessageIdsRef.current?.add(id);
    setData((current) => current ? {
      ...current,
      messages: [...current.messages, optimisticMessage],
    } : current);
    return id;
  }

  function removeOptimisticMessage(messageId: string) {
    knownMessageIdsRef.current?.delete(messageId);
    setData((current) => current ? {
      ...current,
      messages: current.messages.filter((message) => message.id !== messageId),
    } : current);
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
    const body = draft.trim();
    const sendAsTask = channel.kind === "dm" ? false : asTask;
    const optimisticId = addOptimisticOwnerMessage(channel.id, null, body, sendAsTask);
    setDraft("");
    try {
      await invoke("send_message", {
        channelId: channel.id,
        threadRootId: null,
        body,
        asTask: sendAsTask,
      });
      await refresh();
    } catch (err) {
      removeOptimisticMessage(optimisticId);
      setDraft(body);
      const message = errorMessage(err, "Failed to send message");
      setAppError(message);
      console.error(err);
    }
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
    const body = replyDraft.trim();
    const optimisticId = addOptimisticOwnerMessage(channel.id, activeRoot.id, body, false);
    setReplyDraft("");
    try {
      await invoke("send_message", {
        channelId: channel.id,
        threadRootId: activeRoot.id,
        body,
        asTask: false,
      });
      await refresh();
    } catch (err) {
      removeOptimisticMessage(optimisticId);
      setReplyDraft(body);
      const message = errorMessage(err, "Failed to send reply");
      setAppError(message);
      console.error(err);
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
    openThread(task.message_id);
    setActiveTab("chat");
  }

  function openWorkItem(item: AgentWorkItem) {
    if (item.channel_id) setActiveChannelId(item.channel_id);
    if (item.thread_root_id) {
      openThread(item.thread_root_id);
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
      openThread(result.threadId);
      setActiveTab("chat");
    }
    setShowSearchModal(false);
  }

  function openThreadFromBrowser(message: Message) {
    selectChannel(message.channel_id);
    openThread(message.id);
    setActiveTab("chat");
    setShowThread(true);
    setShowThreadBrowserModal(false);
  }

  function startSidebarResize(event: ReactPointerEvent<HTMLButtonElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = sidebarWidth;

    function onPointerMove(moveEvent: PointerEvent) {
      const delta = moveEvent.clientX - startX;
      const maxWidth = Math.min(MAX_SIDEBAR_WIDTH, window.innerWidth - MIN_CONVERSATION_WIDTH - MIN_THREAD_PANEL_WIDTH);
      const next = Math.min(maxWidth, Math.max(MIN_SIDEBAR_WIDTH, startWidth + delta));
      setSidebarWidth(next);
    }

    function onPointerUp() {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      document.body.classList.remove("resizing-column");
    }

    document.body.classList.add("resizing-column");
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
  }

  function startThreadResize(event: ReactPointerEvent<HTMLButtonElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = threadPanelWidth;

    function onPointerMove(moveEvent: PointerEvent) {
      const delta = startX - moveEvent.clientX;
      const maxWidth = Math.max(
        MIN_THREAD_PANEL_WIDTH,
        Math.min(maxThreadPanelWidth(), window.innerWidth - sidebarWidth - MIN_CONVERSATION_WIDTH),
      );
      const next = Math.min(maxWidth, Math.max(MIN_THREAD_PANEL_WIDTH, startWidth + delta));
      setThreadPanelWidth(next);
    }

    function onPointerUp() {
      window.removeEventListener("pointermove", onPointerMove);
      window.removeEventListener("pointerup", onPointerUp);
      document.body.classList.remove("resizing-column");
    }

    document.body.classList.add("resizing-column");
    window.addEventListener("pointermove", onPointerMove);
    window.addEventListener("pointerup", onPointerUp);
  }

  async function toggleThreadFollow(message: Message) {
    if (message.thread_followed) {
      setLocallyUnfollowedThreadIds((current) => new Set(current).add(message.id));
    } else {
      setLocallyUnfollowedThreadIds((current) => {
        if (!current.has(message.id)) return current;
        const next = new Set(current);
        next.delete(message.id);
        return next;
      });
    }
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
    <main
      className={`app theme-liquid ${selectedAgent || showThread ? "" : "thread-hidden"}`}
      style={{
        "--sidebar-width": `${sidebarWidth}px`,
        "--thread-width": `${threadPanelWidth}px`,
      } as CSSProperties}
    >
      <Sidebar
        data={data}
        channel={channel}
        channelAlertIds={channelAlertIds}
        threadUnreadCounts={threadUnreadCounts}
        openSearch={() => setShowSearchModal(true)}
        openThreadBrowser={() => setShowThreadBrowserModal(true)}
        openCreateChannelModal={() => setShowCreateChannelModal(true)}
        openChannelSettingsModal={() => setShowChannelSettingsModal(true)}
        selectChannel={selectChannel}
        openCreateAgentModal={() => setShowCreateAgentModal(true)}
        openAgentDetail={(agent) => setSelectedAgentId(agent.id)}
        openDmWithAgent={openDmWithAgent}
        onResizeStart={startSidebarResize}
      />

      <SearchModal
        open={showSearchModal}
        query={searchQuery}
        scope={searchScope}
        timeRange={searchTimeRange}
        results={searchResults}
        onQueryChange={setSearchQuery}
        onScopeChange={setSearchScope}
        onTimeRangeChange={setSearchTimeRange}
        onOpenResult={openSearchResult}
        onClear={() => setSearchQuery("")}
        onClose={() => setShowSearchModal(false)}
      />

      <ThreadBrowserModal
        open={showThreadBrowserModal}
        channels={data.channels}
        threads={allThreadRootMessages}
        activeThreadId={activeThreadId}
        replyCounts={threadReplyCounts}
        unreadCounts={threadUnreadCounts}
        onOpenThread={openThreadFromBrowser}
        onToggleFollow={toggleThreadFollow}
        onClose={() => setShowThreadBrowserModal(false)}
      />

      <Conversation
        channel={channel}
        agents={data.agents}
        channelAgents={channelAgents}
        activeTab={activeTab}
        activeRoot={activeRoot}
        rootMessages={rootMessages}
        threadReplyCounts={threadReplyCounts}
        visibleTasks={visibleTasks}
        draft={draft}
        taskDraft={taskDraft}
        taskTitleDrafts={taskTitleDrafts}
        showThread={showThread}
        setActiveTab={setActiveTab}
        setActiveThreadId={openThread}
        setShowThread={setShowThread}
        openChannelAgentsModal={() => setShowChannelAgentsModal(true)}
        taskForMessage={taskForMessage}
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
          onCancelWorkItem={cancelWorkItem}
          onRetryWorkItem={retryWorkItem}
        />
      ) : showThread && (
        <ThreadPanel
          channel={channel}
          agents={data.agents}
          activeRoot={activeRoot}
          activeTask={activeTask}
          replies={replies}
          unreadCount={activeThreadId ? threadUnreadCounts[activeThreadId] ?? 0 : 0}
          taskTitleDrafts={taskTitleDrafts}
          replyDraft={replyDraft}
          setActiveThreadId={openThread}
          setTaskTitleDraft={setTaskTitleDraft}
          saveTaskTitle={saveTaskTitle}
          claimTask={claimTask}
          updateTaskStatus={updateTaskStatus}
          setReplyDraft={setReplyDraft}
          sendReply={sendReply}
          onResizeStart={startThreadResize}
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

    </main>
  );
}

createRoot(document.getElementById("root")!).render(<App />);
