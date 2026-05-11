import {
  Component,
  type CSSProperties,
  type ErrorInfo,
  type PointerEvent as ReactPointerEvent,
  type ReactNode,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createRoot } from "react-dom/client";
import { apiInvoke, isTauriRuntime, subscribeBackendEvents } from "./apiClient";
import { AgentDetailDrawer } from "./components/AgentDetailDrawer";
import type { AgentPerformance } from "./components/AgentDetailDrawer";
import { AgentFormModal } from "./components/AgentFormModal";
import { ChannelAgentsModal } from "./components/ChannelAgentsModal";
import { ChannelSettingsModal } from "./components/ChannelSettingsModal";
import { ConfirmModal } from "./components/ConfirmModal";
import { Conversation } from "./components/Conversation";
import { CreateChannelModal } from "./components/CreateChannelModal";
import { InboxModal } from "./components/InboxModal";
import type { RespondingIndicatorItem } from "./components/RespondingIndicator";
import { SavedMessagesModal } from "./components/SavedMessagesModal";
import { SearchModal } from "./components/SearchModal";
import { Sidebar } from "./components/Sidebar";
import { ThreadPanel } from "./components/ThreadPanel";
import { isStreamingMessage } from "./message-grouping";
import {
  ACTIVE_RUN_STATUSES,
  Agent,
  AgentActivity,
  AgentForm,
  AgentRun,
  AgentWorkItem,
  Artifact,
  Bootstrap,
  DraftAttachment,
  EMPTY_AGENT_FORM,
  InboxItem,
  Message,
  RUNTIME_PRESETS,
  RuntimeCheck,
  SavedMessage,
  SearchResult,
  SearchScope,
  SearchTimeRange,
  Task,
} from "./types";
import { agentRequestSourceLabel, buildPresetCommand, firstLines, formatTime, visibleChannelDescription } from "./ui-utils";
import "./styles.css";

const ACTIVITY_PHASE_LABELS: Record<string, string> = {
  thinking: "Thinking",
  command: "Running command",
  file_edit: "Editing file",
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
const DEFAULT_AGENT_DRAWER_WIDTH = 420;
const MIN_AGENT_DRAWER_WIDTH = 320;
const DEFAULT_SIDEBAR_WIDTH = 292;
const MIN_SIDEBAR_WIDTH = 240;
const MAX_SIDEBAR_WIDTH = 460;
const MIN_CONVERSATION_WIDTH = 360;
const MOBILE_BREAKPOINT = 760;
const UI_REFRESH_DEBOUNCE_MS = 80;
const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;
const OWNER_MENTION_HANDLES = ["@Theo", "@Dylan"];

const RESPONDING_ACTIVITY_KINDS = new Set([
  "thinking",
  "command",
  "file_edit",
  "tools",
  "acting",
  "message",
  "event",
  "task",
  "runtime",
  "work",
  "error",
  "event_error",
  "run_error",
]);

type UiBackendEvent =
  | { type: "refresh"; reason?: string }
  | { type: "batch"; events: string[] }
  | { type: "message_upsert"; reason?: string; message: Message }
  | { type: "message_delta"; reason?: string; message_id: string; append: string; delivery_state: Message["delivery_state"] }
  | { type: "message_delete"; reason?: string; message_id: string }
  | { type: "activity_upsert"; reason?: string; activity: AgentActivity }
  | { type: "agent_run_upsert"; reason?: string; run: Omit<AgentRun, "log"> & { log?: string } }
  | { type: "work_item_upsert"; reason?: string; work_item: Omit<AgentWorkItem, "context"> & { context?: string } }
  | { type: "artifact_upsert"; reason?: string; artifact: Artifact };

type ConfirmRequest = {
  title: string;
  body: string;
  confirmLabel: string;
  onConfirm: () => Promise<void> | void;
};

type ActiveTab = "chat" | "tasks";

type MobileHistoryState = {
  __localslockMobileUi: true;
  index: number;
  activeChannelId: string;
  activeThreadId: string | null;
  activeTab: ActiveTab;
  showThread: boolean;
  showMobileSidebar: boolean;
  selectedAgentId: string | null;
};

type AppErrorBoundaryState = {
  error: Error | null;
  info: ErrorInfo | null;
};

function phaseForActivity(kind: string) {
  return ACTIVITY_PHASE_LABELS[kind] ?? "Active";
}

function agentForStreamingMessage(message: Message, agents: Agent[]) {
  if (message.sender_role !== "agent") return null;
  const sender = message.sender_name.replace(/^@/, "");
  return agents.find((agent) => agent.handle === sender || agent.display_name === message.sender_name) ?? null;
}

function activityStateForIndicator(activity: AgentActivity | null) {
  if (!activity) return "Responding";
  if (activity.status === "error") return "Error";

  const title = `${activity.summary || ""} ${activity.title || ""}`.toLowerCase();
  if (title.includes("first token")) return "Responding";
  if (title.includes("running command")) return "Running";
  if (title.includes("editing file")) return "Editing";
  if (title.includes("using tools")) return "Using tools";
  if (title.includes("thinking")) return "Thinking";

  switch (activity.phase || activity.kind) {
    case "thinking":
      return "Thinking";
    case "command":
    case "runtime":
    case "work":
      return "Running";
    case "file_edit":
      return "Editing";
    case "tools":
      return "Using tools";
    case "error":
    case "event_error":
    case "run_error":
      return "Error";
    case "acting":
    case "event":
    case "message":
    case "task":
      return "Responding";
    default:
      return "Working";
  }
}

function activityMatchesAgent(activity: AgentActivity, agent: Agent) {
  return activity.agent_id === agent.id || activity.agent_handle === agent.handle;
}

function isRespondingActivity(activity: AgentActivity) {
  return RESPONDING_ACTIVITY_KINDS.has(activity.phase || activity.kind) || RESPONDING_ACTIVITY_KINDS.has(activity.kind);
}

function latestRespondingActivityForAgent(agent: Agent, activities: AgentActivity[], runId?: string): AgentActivity | null {
  const agentActivities = activities.filter((activity) => activityMatchesAgent(activity, agent));
  const scopedActivities = runId ? agentActivities.filter((activity) => activity.run_id === runId) : agentActivities;
  return scopedActivities.find(isRespondingActivity)
    ?? scopedActivities[0]
    ?? (runId ? latestRespondingActivityForAgent(agent, activities) : null)
    ?? null;
}

function agentForRun(run: AgentRun, agents: Agent[]) {
  return agents.find((agent) => agent.id === run.agent_id || agent.handle === run.agent_handle) ?? null;
}

function respondingIndicatorsForRuns(
  runs: AgentRun[],
  workItems: AgentWorkItem[],
  agents: Agent[],
  activities: AgentActivity[],
  isInContext: (workItem: AgentWorkItem) => boolean,
): RespondingIndicatorItem[] {
  const seen = new Set<string>();
  return runs.flatMap((run) => {
    if (!ACTIVE_RUN_STATUSES.has(run.status)) return [];
    const workItem = run.work_item_id ? workItems.find((item) => item.id === run.work_item_id) : null;
    if (!workItem || !isInContext(workItem)) return [];
    const key = run.agent_id || run.agent_handle;
    if (!key || seen.has(key)) return [];
    seen.add(key);
    const agent = agentForRun(run, agents);
    const activity = agent ? latestRespondingActivityForAgent(agent, activities, run.id) : null;
    return [{
      name: agent?.display_name || run.agent_handle,
      state: activityStateForIndicator(activity),
    }];
  });
}

function mergeRespondingItems(primary: RespondingIndicatorItem[], fallback: RespondingIndicatorItem[]) {
  if (primary.length === 0) return fallback;
  const seen = new Set(primary.map((item) => item.name));
  return [
    ...primary,
    ...fallback.filter((item) => {
      if (seen.has(item.name)) return false;
      seen.add(item.name);
      return true;
    }),
  ];
}

function respondingIndicatorsForMessages(
  messages: Message[],
  agents: Agent[],
  activities: AgentActivity[],
): RespondingIndicatorItem[] {
  const seen = new Set<string>();
  return messages.flatMap((message) => {
    const agent = agentForStreamingMessage(message, agents);
    const key = agent?.id ?? message.sender_name;
    if (!key || seen.has(key)) return [];
    seen.add(key);
    const activity = agent ? latestRespondingActivityForAgent(agent, activities) : null;
    return [{
      name: agent?.display_name || message.sender_name,
      state: activityStateForIndicator(activity),
    }];
  });
}

function isTextInput(target: EventTarget | null) {
  if (!(target instanceof HTMLElement)) return false;
  return ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName) || target.isContentEditable;
}

function isMobileViewport() {
  return window.innerWidth <= MOBILE_BREAKPOINT;
}

function isMobileHistoryState(value: unknown): value is MobileHistoryState {
  if (!value || typeof value !== "object") return false;
  const state = value as Record<string, unknown>;
  return state.__localslockMobileUi === true
    && typeof state.index === "number"
    && typeof state.activeChannelId === "string"
    && (state.activeThreadId === null || typeof state.activeThreadId === "string")
    && (state.activeTab === "chat" || state.activeTab === "tasks")
    && typeof state.showThread === "boolean"
    && typeof state.showMobileSidebar === "boolean"
    && (state.selectedAgentId === null || typeof state.selectedAgentId === "string");
}

function mobileHistoryKey(state: MobileHistoryState) {
  return [
    state.activeChannelId,
    state.activeThreadId ?? "",
    state.activeTab,
    state.showThread ? "thread" : "conversation",
    state.showMobileSidebar ? "sidebar" : "content",
    state.selectedAgentId ?? "",
  ].join("|");
}

function errorMessage(err: unknown, fallback: string) {
  if (err instanceof Error && err.message) return err.message;
  if (typeof err === "string" && err.trim()) return err;
  return fallback;
}

class AppErrorBoundary extends Component<{ children: ReactNode }, AppErrorBoundaryState> {
  state: AppErrorBoundaryState = { error: null, info: null };

  static getDerivedStateFromError(error: Error): AppErrorBoundaryState {
    return { error, info: null };
  }

  componentDidCatch(error: Error, info: ErrorInfo) {
    console.error("LocalSlock UI crashed", error, info);
    this.setState({ info });
  }

  render() {
    if (!this.state.error) return this.props.children;
    const details = [
      this.state.error.stack || this.state.error.message,
      this.state.info?.componentStack,
    ].filter(Boolean).join("\n\n");

    return (
      <main className="fatal-shell">
        <section className="fatal-card" role="alert">
          <p className="eyebrow">LocalSlock UI crashed</p>
          <h1>Frontend render failed</h1>
          <p>
            The backend is still running. Reload the app to recover; the details below are kept
            visible so this does not become a blank window.
          </p>
          <div className="fatal-actions">
            <button type="button" onClick={() => window.location.reload()}>Reload LocalSlock</button>
          </div>
          <pre>{details}</pre>
        </section>
      </main>
    );
  }
}

function maxThreadPanelWidth() {
  return Math.max(MIN_THREAD_PANEL_WIDTH, Math.floor(window.innerWidth * (2 / 3)));
}

function maxAgentDrawerWidth() {
  return Math.max(MIN_AGENT_DRAWER_WIDTH, Math.floor(window.innerWidth * (2 / 3)));
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

function clientId() {
  if (globalThis.crypto && typeof globalThis.crypto.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function draftAttachmentFromFile(file: File): DraftAttachment {
  return {
    id: `${file.name}-${file.size}-${file.lastModified}-${clientId()}`,
    file,
    original_name: file.name,
    mime_type: file.type || "application/octet-stream",
    size_bytes: file.size,
  };
}

type ComposerDraftState = {
  text: string;
  attachments: DraftAttachment[];
};

const EMPTY_COMPOSER_DRAFT: ComposerDraftState = {
  text: "",
  attachments: [],
};

function isEmptyComposerDraft(draft: ComposerDraftState) {
  return draft.text.length === 0 && draft.attachments.length === 0;
}

function updateComposerDraftRecord(
  current: Record<string, ComposerDraftState>,
  key: string | null | undefined,
  updater: (draft: ComposerDraftState) => ComposerDraftState,
) {
  if (!key) return current;
  const previous = current[key] ?? EMPTY_COMPOSER_DRAFT;
  const nextDraft = updater(previous);
  const next = { ...current };
  if (isEmptyComposerDraft(nextDraft)) {
    delete next[key];
  } else {
    next[key] = nextDraft;
  }
  return next;
}

async function attachmentUploads(attachments: DraftAttachment[]) {
  return Promise.all(attachments.map(async (attachment) => {
    const buffer = await attachment.file.arrayBuffer();
    return {
      originalName: attachment.original_name,
      mimeType: attachment.mime_type,
      bytes: Array.from(new Uint8Array(buffer)),
    };
  }));
}

function defaultAgentWorkspace(handle: string) {
  const normalized = handle.trim().replace(/^@/, "").replace(/[^A-Za-z0-9_-]/g, "-");
  return normalized ? `/Users/dylan/Desktop/workspace/localslock/agents/${normalized}` : "";
}

function numericMetadata(value: unknown) {
  if (typeof value === "number" && Number.isFinite(value)) return value;
  if (typeof value === "string") {
    const parsed = Number.parseFloat(value);
    return Number.isFinite(parsed) ? parsed : null;
  }
  return null;
}

function percentile(values: number[], ratio: number) {
  if (values.length === 0) return null;
  const sorted = [...values].sort((left, right) => left - right);
  const index = Math.min(sorted.length - 1, Math.ceil(sorted.length * ratio) - 1);
  return sorted[index];
}

function messageMentionsOwner(message: Message) {
  const body = message.body.toLowerCase();
  return OWNER_MENTION_HANDLES.some((handle) => body.includes(handle.toLowerCase()));
}

function budgetMicrosFromForm(value: string) {
  const parsed = Number.parseFloat(value);
  if (!Number.isFinite(parsed) || parsed <= 0) return 0;
  return Math.round(parsed * 1_000_000);
}

function budgetUsdFromMicros(value: number) {
  return value > 0 ? (value / 1_000_000).toFixed(2) : "";
}

function buildAgentPerformance(activities: AgentActivity[], runs: AgentRun[]): AgentPerformance {
  const cutoff = Date.now() - 24 * 60 * 60 * 1000;
  const recent = activities.filter((activity) => {
    const timestamp = new Date(activity.created_at).getTime();
    return Number.isNaN(timestamp) || timestamp >= cutoff;
  });
  const recentRuns = runs.filter((run) => {
    const timestamp = new Date(run.started_at).getTime();
    return Number.isNaN(timestamp) || timestamp >= cutoff;
  });
  const firstTokenMs = recent
    .map((activity) => numericMetadata(activity.metadata.first_token_ms))
    .filter((value): value is number => value !== null);
  const finishedTurns = recent.filter((activity) =>
    activity.phase === "runtime" &&
    ["Completed", "Failed", "Stopped"].includes(activity.title || activity.summary));
  const turnDurations = finishedTurns
    .filter((activity) => activity.status === "success")
    .map((activity) => numericMetadata(activity.metadata.duration_ms))
    .filter((value): value is number => value !== null);
  const failedTurns = finishedTurns.filter((activity) => activity.status === "error").length;
  const completedTurns = finishedTurns.filter((activity) => activity.status === "success").length;
  const activeTurns = recent.filter((activity) =>
    activity.phase === "runtime" &&
    activity.status === "active" &&
    (activity.title === "Started working" || activity.summary === "Started working")).length;
  const turns = completedTurns + failedTurns + activeTurns;
  const inputTokens = recentRuns.reduce((total, run) => total + (run.input_tokens || 0), 0);
  const outputTokens = recentRuns.reduce((total, run) => total + (run.output_tokens || 0), 0);
  const costMicros = recentRuns.reduce((total, run) => total + (run.cost_micros || 0), 0);

  return {
    windowLabel: "Last 24h",
    turns,
    completedTurns,
    failedTurns,
    activeTurns,
    p50FirstTokenMs: percentile(firstTokenMs, 0.5),
    p95FirstTokenMs: percentile(firstTokenMs, 0.95),
    p50TurnMs: percentile(turnDurations, 0.5),
    p95TurnMs: percentile(turnDurations, 0.95),
    errorRate: completedTurns + failedTurns === 0 ? 0 : failedTurns / (completedTurns + failedTurns),
    inputTokens,
    outputTokens,
    costMicros,
  };
}

function App() {
  const [data, setData] = useState<Bootstrap | null>(null);
  const [activeChannelId, setActiveChannelId] = useState<string>("");
  const [activeThreadId, setActiveThreadId] = useState<string | null>(null);
  const [activeTab, setActiveTab] = useState<ActiveTab>("chat");
  const [rootComposerDrafts, setRootComposerDrafts] = useState<Record<string, ComposerDraftState>>({});
  const [replyComposerDrafts, setReplyComposerDrafts] = useState<Record<string, ComposerDraftState>>({});
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
  const [showThread, setShowThread] = useState(() => window.innerWidth > MOBILE_BREAKPOINT);
  const [showCreateChannelModal, setShowCreateChannelModal] = useState(false);
  const [showChannelSettingsModal, setShowChannelSettingsModal] = useState(false);
  const [showChannelAgentsModal, setShowChannelAgentsModal] = useState(false);
  const [showCreateAgentModal, setShowCreateAgentModal] = useState(false);
  const [showSearchModal, setShowSearchModal] = useState(false);
  const [showInboxModal, setShowInboxModal] = useState(false);
  const [showSavedModal, setShowSavedModal] = useState(false);
  const [showMobileSidebar, setShowMobileSidebar] = useState(false);
  const [selectedAgentId, setSelectedAgentId] = useState<string | null>(null);
  const [focusedMessageId, setFocusedMessageId] = useState<string | null>(null);
  const [appError, setAppError] = useState<string | null>(null);
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);
  const [runtimeChecks, setRuntimeChecks] = useState<Record<string, RuntimeCheck>>({});
  const [threadPanelWidth, setThreadPanelWidth] = useState(() => {
    const stored = window.localStorage.getItem("localslock.threadPanelWidth");
    const value = stored ? Number(stored) : DEFAULT_THREAD_PANEL_WIDTH;
    return Number.isFinite(value)
      ? Math.min(maxThreadPanelWidth(), Math.max(MIN_THREAD_PANEL_WIDTH, value))
      : DEFAULT_THREAD_PANEL_WIDTH;
  });
  const [agentDrawerWidth, setAgentDrawerWidth] = useState(() => {
    const stored = window.localStorage.getItem("localslock.agentDrawerWidth");
    const value = stored ? Number(stored) : DEFAULT_AGENT_DRAWER_WIDTH;
    return Number.isFinite(value)
      ? Math.min(maxAgentDrawerWidth(), Math.max(MIN_AGENT_DRAWER_WIDTH, value))
      : DEFAULT_AGENT_DRAWER_WIDTH;
  });
  const [sidebarWidth, setSidebarWidth] = useState(() => {
    const stored = window.localStorage.getItem("localslock.sidebarWidth");
    const value = stored ? Number(stored) : DEFAULT_SIDEBAR_WIDTH;
    return Number.isFinite(value)
      ? Math.min(MAX_SIDEBAR_WIDTH, Math.max(MIN_SIDEBAR_WIDTH, value))
      : DEFAULT_SIDEBAR_WIDTH;
  });
  const rootComposerDraft = activeChannelId ? rootComposerDrafts[activeChannelId] ?? EMPTY_COMPOSER_DRAFT : EMPTY_COMPOSER_DRAFT;
  const replyComposerDraft = activeThreadId ? replyComposerDrafts[activeThreadId] ?? EMPTY_COMPOSER_DRAFT : EMPTY_COMPOSER_DRAFT;
  const draft = rootComposerDraft.text;
  const draftAttachments = rootComposerDraft.attachments;
  const replyDraft = replyComposerDraft.text;
  const replyAttachments = replyComposerDraft.attachments;

  useEffect(() => {
    if (!focusedMessageId) return;
    const timer = window.setTimeout(() => setFocusedMessageId(null), 2600);
    return () => window.clearTimeout(timer);
  }, [focusedMessageId]);
  const [channelAlertIds, setChannelAlertIds] = useState<Set<string>>(() => new Set());
  const [threadUnreadCounts, setThreadUnreadCounts] = useState<Record<string, number>>({});
  const [dismissedInboxItems, setDismissedInboxItems] = useState<Record<string, string>>({});
  const [locallyUnfollowedThreadIds, setLocallyUnfollowedThreadIds] = useState<Set<string>>(() => new Set());
  const knownMessageIdsRef = useRef<Set<string> | null>(null);
  const refreshTimerRef = useRef<number | null>(null);
  const refreshInFlightRef = useRef(false);
  const refreshQueuedRef = useRef(false);
  const messageDeltaBufferRef = useRef<Map<string, { append: string; deliveryState: Message["delivery_state"] }>>(new Map());
  const messageDeltaFlushTimerRef = useRef<number | null>(null);
  const mobileHistoryReadyRef = useRef(false);
  const mobileHistoryIndexRef = useRef(0);
  const restoringMobileHistoryRef = useRef(false);
  const lastMobileHistoryKeyRef = useRef<string | null>(null);

  function buildMobileHistoryState(index = mobileHistoryIndexRef.current): MobileHistoryState {
    return {
      __localslockMobileUi: true,
      index,
      activeChannelId,
      activeThreadId,
      activeTab,
      showThread,
      showMobileSidebar,
      selectedAgentId,
    };
  }

  async function refreshRuntimeChecks() {
    if (!isTauriRuntime()) {
      setRuntimeChecks({});
      return;
    }
    const entries = await Promise.all(
      Object.keys(RUNTIME_PRESETS).map(async (runtime) => {
        const check = await apiInvoke<RuntimeCheck>("check_runtime", { runtime });
        return [runtime, check] as const;
      }),
    );
    setRuntimeChecks(Object.fromEntries(entries));
  }

  async function refresh() {
    const next = await apiInvoke<Bootstrap>("bootstrap");
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
    messageDeltaBufferRef.current.delete(message.id);
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

  function flushMessageDeltas() {
    if (messageDeltaFlushTimerRef.current !== null) {
      window.clearTimeout(messageDeltaFlushTimerRef.current);
      messageDeltaFlushTimerRef.current = null;
    }
    if (messageDeltaBufferRef.current.size === 0) return;
    const deltas = messageDeltaBufferRef.current;
    messageDeltaBufferRef.current = new Map();
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after message delta");
        return current;
      }
      let missing = false;
      let changed = false;
      const messages = current.messages.map((item) => {
        const delta = deltas.get(item.id);
        if (!delta) return item;
        changed = true;
        return { ...item, body: `${item.body}${delta.append}`, delivery_state: delta.deliveryState };
      });
      for (const messageId of deltas.keys()) {
        if (!current.messages.some((item) => item.id === messageId)) {
          missing = true;
          break;
        }
      }
      if (missing) {
        requestRefresh("Failed to refresh LocalSlock state after message delta");
      }
      if (!changed) return current;
      return { ...current, messages };
    });
  }

  function queueMessageDelta(messageId: string, append: string, deliveryState: Message["delivery_state"]) {
    const existing = messageDeltaBufferRef.current.get(messageId);
    messageDeltaBufferRef.current.set(messageId, {
      append: `${existing?.append ?? ""}${append}`,
      deliveryState,
    });
    if (messageDeltaFlushTimerRef.current !== null) return;
    messageDeltaFlushTimerRef.current = window.setTimeout(() => {
      flushMessageDeltas();
    }, 50);
  }

  function applyMessageDelete(messageId: string) {
    messageDeltaBufferRef.current.delete(messageId);
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after message deletion");
        return current;
      }
      return { ...current, messages: current.messages.filter((item) => item.id !== messageId) };
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
        requestRefresh("Failed to refresh LocalSlock state after agent request update");
        return current;
      }
      const existing = current.agent_work_items.find((item) => item.id === patch.id);
      const workItem: AgentWorkItem = {
        ...patch,
        context: patch.context ?? existing?.context ?? "",
        source_kind: patch.source_kind ?? existing?.source_kind ?? "manual",
      };
      const agent_work_items = existing
        ? current.agent_work_items.map((item) => item.id === patch.id ? { ...item, ...workItem } : item)
        : [workItem, ...current.agent_work_items];
      agent_work_items.sort((left, right) => new Date(right.created_at).getTime() - new Date(left.created_at).getTime());
      return { ...current, agent_work_items: agent_work_items.slice(0, 80) };
    });
  }

  function applyArtifactUpsert(artifact: Artifact) {
    if (!artifact || typeof artifact.id !== "string" || typeof artifact.message_id !== "string") {
      requestRefresh("Failed to refresh LocalSlock state after artifact update");
      return;
    }
    setData((current) => {
      if (!current) {
        requestRefresh("Failed to refresh LocalSlock state after artifact update");
        return current;
      }
      const currentArtifacts = Array.isArray(current.artifacts) ? current.artifacts : [];
      const existingIndex = currentArtifacts.findIndex((item) => item.id === artifact.id);
      const artifacts = existingIndex >= 0
        ? currentArtifacts.map((item) => item.id === artifact.id ? artifact : item)
        : [...currentArtifacts, artifact];
      const messages = current.messages.map((message) => {
        if (message.id !== artifact.message_id) return message;
        const currentMessageArtifacts = Array.isArray(message.artifacts) ? message.artifacts : [];
        const existingArtifactIndex = currentMessageArtifacts.findIndex((item) => item.id === artifact.id);
        const messageArtifacts = existingArtifactIndex >= 0
          ? currentMessageArtifacts.map((item) => item.id === artifact.id ? artifact : item)
          : [...currentMessageArtifacts, artifact];
        return { ...message, artifacts: messageArtifacts };
      });
      return { ...current, artifacts, messages };
    });
  }

  async function openArtifact(artifact: Artifact) {
    try {
      const fullArtifact = await apiInvoke<Artifact>("artifact_read", { artifactId: artifact.id });
      const blob = new Blob([fullArtifact.content], { type: "text/plain;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      window.open(url, "_blank", "noopener,noreferrer");
      window.setTimeout(() => URL.revokeObjectURL(url), 30_000);
    } catch (err) {
      setAppError(errorMessage(err, "Failed to open artifact"));
    }
  }

  function handleBackendEvent(payload: unknown) {
    try {
      if (typeof payload !== "string") {
        requestRefresh("Failed to refresh LocalSlock state after backend update");
        return;
      }
      const parsed = JSON.parse(payload) as UiBackendEvent;
      if (parsed.type === "batch") {
        for (const eventPayload of parsed.events) {
          handleBackendEvent(eventPayload);
        }
        return;
      }
      if (parsed.type === "message_upsert") {
        applyMessageUpsert(parsed.message);
        return;
      }
      if (parsed.type === "message_delta") {
        queueMessageDelta(parsed.message_id, parsed.append, parsed.delivery_state);
        return;
      }
      if (parsed.type === "message_delete") {
        applyMessageDelete(parsed.message_id);
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
      if (parsed.type === "artifact_upsert") {
        applyArtifactUpsert(parsed.artifact);
        return;
      }
      requestRefresh("Failed to refresh LocalSlock state after backend update");
    } catch (err) {
      setAppError(errorMessage(err, "Failed to apply LocalSlock backend update"));
      console.error("Failed to apply LocalSlock backend update", err, payload);
      requestRefresh("Failed to refresh LocalSlock state after backend update");
    }
  }

  async function mutate(command: string, args: Record<string, unknown> = {}) {
    try {
      await apiInvoke(command, args);
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
    function isFileDrag(event: DragEvent) {
      return Array.from(event.dataTransfer?.types ?? []).includes("Files");
    }

    function preventFileNavigation(event: DragEvent) {
      if (!isFileDrag(event)) return;
      event.preventDefault();
    }

    window.addEventListener("dragover", preventFileNavigation);
    window.addEventListener("drop", preventFileNavigation);
    return () => {
      window.removeEventListener("dragover", preventFileNavigation);
      window.removeEventListener("drop", preventFileNavigation);
    };
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | null = null;
    subscribeBackendEvents(handleBackendEvent)
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
      if (messageDeltaFlushTimerRef.current !== null) {
        window.clearTimeout(messageDeltaFlushTimerRef.current);
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
      knownMessageIdsRef.current = new Set(data.messages.filter((message) => !isStreamingMessage(message)).map((message) => message.id));
      return;
    }

    const known = knownMessageIdsRef.current;
    const newMessages = data.messages.filter((message) => !isStreamingMessage(message) && !known.has(message.id));
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
    window.localStorage.setItem("localslock.agentDrawerWidth", String(agentDrawerWidth));
  }, [agentDrawerWidth]);

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
        showInboxModal ||
        showSavedModal ||
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
    showInboxModal,
    showSavedModal,
    showSearchModal,
    showThread,
  ]);

  const channel = useMemo(() => {
    return data?.channels.find((c) => c.id === activeChannelId) ?? data?.channels[0] ?? null;
  }, [activeChannelId, data]);

  useEffect(() => {
    if (!data || !window.location.hash.startsWith("#/message/")) return;
    const messageId = decodeURIComponent(window.location.hash.replace("#/message/", ""));
    const message = data.messages.find((item) => item.id === messageId || item.id.startsWith(messageId));
    if (!message) return;
    selectChannel(message.channel_id);
    revealThread(message.thread_root_id ?? message.id);
    setFocusedMessageId(message.id);
    setActiveTab("chat");
    window.history.replaceState(window.history.state, "", `${window.location.pathname}${window.location.search}`);
  }, [data]);

  useEffect(() => {
    function onPopState(event: PopStateEvent) {
      if (!isMobileHistoryState(event.state)) return;
      restoringMobileHistoryRef.current = true;
      mobileHistoryReadyRef.current = true;
      mobileHistoryIndexRef.current = event.state.index;
      lastMobileHistoryKeyRef.current = mobileHistoryKey(event.state);
      setActiveChannelId(event.state.activeChannelId);
      setActiveThreadId(event.state.activeThreadId);
      setActiveTab(event.state.activeTab);
      setShowThread(event.state.showThread);
      setShowMobileSidebar(event.state.showMobileSidebar);
      setSelectedAgentId(event.state.selectedAgentId);
    }

    window.addEventListener("popstate", onPopState);
    return () => window.removeEventListener("popstate", onPopState);
  }, []);

  useEffect(() => {
    if (!data || !activeChannelId || !isMobileViewport()) return;

    const currentState = buildMobileHistoryState(mobileHistoryIndexRef.current);
    const currentKey = mobileHistoryKey(currentState);

    if (restoringMobileHistoryRef.current) {
      restoringMobileHistoryRef.current = false;
      lastMobileHistoryKeyRef.current = currentKey;
      return;
    }

    if (!mobileHistoryReadyRef.current) {
      const existingState = window.history.state;
      if (isMobileHistoryState(existingState)) {
        mobileHistoryReadyRef.current = true;
        mobileHistoryIndexRef.current = existingState.index;
        const nextState = buildMobileHistoryState(existingState.index);
        window.history.replaceState(nextState, "");
        lastMobileHistoryKeyRef.current = mobileHistoryKey(nextState);
        return;
      }

      const baseState: MobileHistoryState = {
        ...currentState,
        index: 0,
        activeThreadId: null,
        activeTab: "chat",
        showThread: false,
        showMobileSidebar: true,
        selectedAgentId: null,
      };
      const baseKey = mobileHistoryKey(baseState);
      window.history.replaceState(baseState, "");
      mobileHistoryReadyRef.current = true;
      mobileHistoryIndexRef.current = 0;
      lastMobileHistoryKeyRef.current = baseKey;

      if (baseKey === currentKey) return;
      const firstState = { ...currentState, index: 1 };
      window.history.pushState(firstState, "");
      mobileHistoryIndexRef.current = 1;
      lastMobileHistoryKeyRef.current = mobileHistoryKey(firstState);
      return;
    }

    if (lastMobileHistoryKeyRef.current === currentKey) return;
    const nextState = { ...currentState, index: mobileHistoryIndexRef.current + 1 };
    window.history.pushState(nextState, "");
    mobileHistoryIndexRef.current = nextState.index;
    lastMobileHistoryKeyRef.current = mobileHistoryKey(nextState);
  }, [
    activeChannelId,
    activeTab,
    activeThreadId,
    data,
    selectedAgentId,
    showMobileSidebar,
    showThread,
  ]);

  const visibleMessages = useMemo(() => {
    if (!data) return [];
    return data.messages.filter((message) => !isStreamingMessage(message));
  }, [data?.messages]);

  const streamingMessages = useMemo(() => {
    if (!data) return [];
    return data.messages.filter(isStreamingMessage);
  }, [data?.messages]);

  const rootMessages = useMemo(() => {
    if (!channel) return [];
    return visibleMessages.filter((m) => m.channel_id === channel.id && !m.thread_root_id);
  }, [visibleMessages, channel]);

  const activeRoot = activeThreadId ? rootMessages.find((m) => m.id === activeThreadId) ?? null : null;

  const replies = useMemo(() => {
    if (!activeRoot) return [];
    return visibleMessages.filter((m) => m.thread_root_id === activeRoot.id);
  }, [visibleMessages, activeRoot]);

  const channelRespondingAgents = useMemo(() => {
    if (!channel || !data) return [];
    const runningItems = respondingIndicatorsForRuns(
      data.agent_runs,
      data.agent_work_items,
      data.agents,
      data.agent_activities,
      (workItem) => workItem.channel_id === channel.id && !workItem.thread_root_id,
    );
    const streamingItems = respondingIndicatorsForMessages(
      streamingMessages.filter((message) => message.channel_id === channel.id && !message.thread_root_id),
      data.agents,
      data.agent_activities,
    );
    return mergeRespondingItems(runningItems, streamingItems);
  }, [streamingMessages, channel, data?.agent_runs, data?.agent_work_items, data?.agents, data?.agent_activities]);

  const threadRespondingAgents = useMemo(() => {
    if (!activeRoot || !data) return [];
    const runningItems = respondingIndicatorsForRuns(
      data.agent_runs,
      data.agent_work_items,
      data.agents,
      data.agent_activities,
      (workItem) => workItem.thread_root_id === activeRoot.id,
    );
    const streamingItems = respondingIndicatorsForMessages(
      streamingMessages.filter((message) => message.thread_root_id === activeRoot.id),
      data.agents,
      data.agent_activities,
    );
    return mergeRespondingItems(runningItems, streamingItems);
  }, [streamingMessages, activeRoot, data?.agent_runs, data?.agent_work_items, data?.agents, data?.agent_activities]);

  const threadReplyCounts = useMemo(() => {
    return visibleMessages.reduce<Record<string, number>>((counts, message) => {
      if (!message.thread_root_id) return counts;
      counts[message.thread_root_id] = (counts[message.thread_root_id] ?? 0) + 1;
      return counts;
    }, {});
  }, [visibleMessages]);

  const allThreadRootMessages = useMemo(() => {
    const latestByRoot = new Map<string, number>();
    for (const message of visibleMessages) {
      if (!message.thread_root_id) continue;
      const timestamp = new Date(message.created_at).getTime();
      latestByRoot.set(message.thread_root_id, Math.max(latestByRoot.get(message.thread_root_id) ?? 0, timestamp));
    }
    return visibleMessages
      .filter((message) =>
        !message.thread_root_id &&
        latestByRoot.has(message.id) &&
        (message.thread_followed || (threadUnreadCounts[message.id] ?? 0) > 0) &&
        !locallyUnfollowedThreadIds.has(message.id))
      .sort((left, right) => (latestByRoot.get(right.id) ?? 0) - (latestByRoot.get(left.id) ?? 0));
  }, [visibleMessages, locallyUnfollowedThreadIds, threadUnreadCounts]);

  const inboxItems = useMemo(() => {
    if (!data) return [];
    const channelsById = new Map(data.channels.map((item) => [item.id, item]));
    const agentsById = new Map(data.agents.map((item) => [item.id, item]));
    const latestByChannel = new Map<string, Message>();
    const latestReplyByRoot = new Map<string, Message>();

    for (const message of visibleMessages) {
      const currentChannelLatest = latestByChannel.get(message.channel_id);
      if (!currentChannelLatest || new Date(message.created_at) > new Date(currentChannelLatest.created_at)) {
        latestByChannel.set(message.channel_id, message);
      }
      if (message.thread_root_id) {
        const currentThreadLatest = latestReplyByRoot.get(message.thread_root_id);
        if (!currentThreadLatest || new Date(message.created_at) > new Date(currentThreadLatest.created_at)) {
          latestReplyByRoot.set(message.thread_root_id, message);
        }
      }
    }

    const channelLabel = (channelId: string | null) => {
      if (!channelId) return "LocalSlock";
      const target = channelsById.get(channelId);
      if (!target) return "Unknown";
      if (target.kind === "dm") {
        const agent = target.dm_agent_id ? agentsById.get(target.dm_agent_id) : null;
        return agent ? `@${agent.handle}` : "Direct message";
      }
      return `#${target.name}`;
    };
    const timestamp = (value: string | null | undefined) => value || new Date(0).toISOString();
    const items: InboxItem[] = [];

    for (const channel of data.channels) {
      const unread = channel.unread_count > 0 || channelAlertIds.has(channel.id);
      if (!unread) continue;
      const latest = latestByChannel.get(channel.id);
      const dmAgent = channel.kind === "dm" && channel.dm_agent_id ? agentsById.get(channel.dm_agent_id) : null;
      items.push({
        id: `${channel.kind}:${channel.id}`,
        kind: channel.kind === "dm" ? "dm" : "channel",
        title: channel.kind === "dm" ? `DM with @${dmAgent?.handle ?? "agent"}` : `New activity in #${channel.name}`,
        excerpt: latest?.body ?? visibleChannelDescription(channel.description),
        surface: channel.kind === "dm" ? "Direct message" : `#${channel.name}`,
        actor: latest?.sender_name ?? "",
        timestamp: timestamp(latest?.created_at),
        unread: true,
        channelId: channel.id,
        threadId: latest?.thread_root_id ?? null,
        messageId: latest?.id ?? null,
        taskId: null,
        reminderId: null,
        replyCount: latest?.thread_root_id ? (threadReplyCounts[latest.thread_root_id] ?? 0) : 0,
        newCount: channel.unread_count,
      });
    }

    for (const root of allThreadRootMessages) {
      const latestReply = latestReplyByRoot.get(root.id);
      const unread = (threadUnreadCounts[root.id] ?? 0) > 0;
      items.push({
        id: `thread:${root.id}`,
        kind: "thread",
        title: firstLines(root.body, 1),
        excerpt: latestReply ? `${latestReply.sender_name}: ${latestReply.body}` : root.body,
        surface: channelLabel(root.channel_id),
        actor: root.sender_name,
        timestamp: timestamp(latestReply?.created_at ?? root.created_at),
        unread,
        channelId: root.channel_id,
        threadId: root.id,
        messageId: root.id,
        taskId: null,
        reminderId: null,
        replyCount: threadReplyCounts[root.id] ?? 0,
        newCount: threadUnreadCounts[root.id] ?? 0,
      });
    }

    visibleMessages
      .filter((message) => message.sender_role !== "owner" && messageMentionsOwner(message))
      .sort((left, right) => new Date(right.created_at).getTime() - new Date(left.created_at).getTime())
      .slice(0, 24)
      .forEach((message) => {
        const rootId = message.thread_root_id ?? message.id;
        items.push({
          id: `mention:${message.id}`,
          kind: "mention",
          title: firstLines(message.body, 1),
          excerpt: message.body,
          surface: channelLabel(message.channel_id),
          actor: message.sender_name,
          timestamp: message.created_at,
          unread: channelAlertIds.has(message.channel_id) || (message.thread_root_id ? (threadUnreadCounts[message.thread_root_id] ?? 0) > 0 : false),
          channelId: message.channel_id,
          threadId: rootId,
          messageId: message.id,
          taskId: null,
          reminderId: null,
          replyCount: threadReplyCounts[rootId] ?? 0,
          newCount: message.thread_root_id ? (threadUnreadCounts[message.thread_root_id] ?? 0) : 0,
        });
      });

    data.tasks
      .filter((task) => task.status !== "done")
      .slice(0, 24)
      .forEach((task) => {
        items.push({
          id: `task:${task.id}`,
          kind: "task",
          title: `Task #${task.number}: ${task.title}`,
          excerpt: task.assignee_name ? `Assigned to ${task.assignee_name}` : "Unassigned",
          surface: `#${task.channel_name}`,
          actor: task.status.replace("_", " "),
          timestamp: task.updated_at,
          unread: task.status === "in_review",
          channelId: task.channel_id,
          threadId: task.message_id,
          messageId: task.message_id,
          taskId: task.id,
          reminderId: null,
          replyCount: threadReplyCounts[task.message_id] ?? 0,
          newCount: 0,
        });
      });

    data.reminders
      .filter((reminder) => reminder.status === "fired")
      .forEach((reminder) => {
        items.push({
          id: `reminder:${reminder.id}`,
          kind: "reminder",
          title: reminder.title,
          excerpt: reminder.note,
          surface: reminder.channel_id ? channelLabel(reminder.channel_id) : "Reminder",
          actor: "Reminder due",
          timestamp: reminder.fired_at ?? reminder.due_at,
          unread: true,
          channelId: reminder.channel_id,
          threadId: reminder.thread_root_id,
          messageId: reminder.message_id,
          taskId: null,
          reminderId: reminder.id,
          replyCount: reminder.thread_root_id ? (threadReplyCounts[reminder.thread_root_id] ?? 0) : 0,
          newCount: 1,
        });
      });

    const kindPriority: Record<InboxItem["kind"], number> = {
      reminder: 0,
      mention: 1,
      dm: 2,
      thread: 3,
      task: 4,
      channel: 5,
    };
    return items
      .filter((item) => {
        const dismissedAt = dismissedInboxItems[item.id];
        if (!dismissedAt) return true;
        return new Date(item.timestamp).getTime() > new Date(dismissedAt).getTime();
      })
      .sort((left, right) => {
        if (left.unread !== right.unread) return left.unread ? -1 : 1;
        const priority = kindPriority[left.kind] - kindPriority[right.kind];
        if (priority !== 0) return priority;
        return new Date(right.timestamp).getTime() - new Date(left.timestamp).getTime();
      })
      .slice(0, 120);
  }, [allThreadRootMessages, channelAlertIds, data, dismissedInboxItems, threadReplyCounts, threadUnreadCounts, visibleMessages]);

  const inboxUnreadCount = useMemo(() => {
    return inboxItems.filter((item) => item.unread).length;
  }, [inboxItems]);

  const savedMessageIds = useMemo(() => {
    return new Set(data?.saved_messages.map((item) => item.message_id) ?? []);
  }, [data?.saved_messages]);

  const shareBaseUrl = useMemo(() => {
    if (!data) return window.location.origin;
    return isTauriRuntime() ? data.web_base_url ?? window.location.origin : window.location.origin;
  }, [data?.web_base_url]);

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
    if (!activeChannelId) return 0;
    return visibleMessages.filter((message) => message.channel_id === activeChannelId).length;
  }, [visibleMessages, activeChannelId]);

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

  const selectedAgentRuns = useMemo(() => {
    if (!data || !selectedAgent) return [];
    return data.agent_runs.filter((run) => run.agent_id === selectedAgent.id);
  }, [data, selectedAgent]);

  const selectedAgentPerformance = useMemo(() => {
    return buildAgentPerformance(selectedAgentActivities, selectedAgentRuns);
  }, [selectedAgentActivities, selectedAgentRuns]);

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
    return data.agent_work_items
      .filter((item) => item.agent_id === selectedAgent.id && item.status !== "silent")
      .slice(0, 6);
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
          return includes(`${item.name} ${visibleChannelDescription(item.description)} ${dmAgent?.handle ?? ""} ${dmAgent?.display_name ?? ""}`);
        })
        .map((item) => {
        const dmAgent = item.kind === "dm" ? data.agents.find((agent) => agent.id === item.dm_agent_id) : null;
        return {
          id: item.id,
          kind: item.kind === "dm" ? "dm" : "channel",
          title: item.kind === "dm" ? `@${dmAgent?.handle ?? "agent"}` : `#${item.name}`,
          detail: item.kind === "dm" ? dmAgent?.display_name ?? "direct message" : visibleChannelDescription(item.description) || "channel",
          excerpt: item.kind === "dm" ? dmAgent?.description ?? "" : visibleChannelDescription(item.description),
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
      results.push(...visibleMessages
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

    if (searchScopeAllows(searchScope, "artifacts")) {
      results.push(...data.artifacts
        .filter((item) =>
          matchesSearchTime(item.created_at, searchTimeRange) &&
          includes(`${item.title} ${item.summary} ${item.content} ${item.kind} ${channelLabel(item.channel_id)}`))
        .sort((a, b) => new Date(b.created_at).getTime() - new Date(a.created_at).getTime())
        .map((item) => ({
        id: item.id,
        kind: "artifact",
        title: item.title,
        detail: `${item.kind} · ${channelLabel(item.channel_id)} · ${formatTime(item.created_at)}`,
        excerpt: firstLines(item.summary || item.content, 2),
        createdAt: item.created_at,
        channelId: item.channel_id,
        threadId: item.thread_root_id ?? item.message_id,
        agentId: item.creator_agent_id,
      })).slice(0, 20));
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
          item.status !== "silent" &&
          matchesSearchTime(item.updated_at, searchTimeRange) &&
          includes(`${item.agent_handle} ${item.status} ${item.title} ${item.context}`))
        .sort((a, b) => new Date(b.updated_at).getTime() - new Date(a.updated_at).getTime())
        .map((item) => ({
        id: item.id,
        kind: "request",
        title: item.title,
        detail: `${agentRequestSourceLabel(item.source_kind, item.task_number)} · ${item.agent_handle} · ${item.status} · ${channelLabel(item.channel_id)}`,
        excerpt: firstLines(item.context, 2),
        createdAt: item.updated_at,
        channelId: item.channel_id,
        threadId: item.thread_root_id,
        agentId: item.agent_id,
      })).slice(0, 16));
    }

    return results.slice(0, 80);
  }, [data, searchQuery, searchScope, searchTimeRange, visibleMessages]);

  function taskForMessage(messageId: string) {
    return data?.tasks.find((task) => task.message_id === messageId) ?? null;
  }

  useEffect(() => {
    setChannelNameDraft(channel?.name ?? "");
    setChannelDescriptionDraft(channel ? visibleChannelDescription(channel.description) : "");
  }, [channel?.id, channel?.name, channel?.description]);

  useEffect(() => {
    if (channel?.kind !== "dm") return;
    setActiveTab("chat");
    setShowChannelSettingsModal(false);
    setShowChannelAgentsModal(false);
  }, [channel?.id, channel?.kind]);

  useEffect(() => {
    if (!activeChannelId) return;
    apiInvoke("mark_channel_read", { channelId: activeChannelId }).catch((err) => console.error(err));
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
    const channelToDelete = channel;
    const fallbackChannelId = data?.channels.find((item) => item.id !== channelToDelete.id)?.id ?? "";
    setConfirmRequest({
      title: `Delete #${channelToDelete.name}?`,
      body: "This removes the channel timeline, tasks, threads, agent memberships, schedules, and attachments for this channel. This cannot be undone.",
      confirmLabel: "Delete channel",
      onConfirm: async () => {
        await mutate("delete_channel", { channelId: channelToDelete.id });
        setShowChannelSettingsModal(false);
        if (activeChannelId === channelToDelete.id) {
          setActiveChannelId(fallbackChannelId);
          setActiveThreadId(null);
        }
      },
    });
  }

  function updateRootComposerDraft(channelId: string | null | undefined, updater: (draft: ComposerDraftState) => ComposerDraftState) {
    setRootComposerDrafts((current) => updateComposerDraftRecord(current, channelId, updater));
  }

  function updateReplyComposerDraft(threadId: string | null | undefined, updater: (draft: ComposerDraftState) => ComposerDraftState) {
    setReplyComposerDrafts((current) => updateComposerDraftRecord(current, threadId, updater));
  }

  function setDraft(value: string) {
    updateRootComposerDraft(activeChannelId, (current) => ({ ...current, text: value }));
  }

  function setReplyDraft(value: string) {
    updateReplyComposerDraft(activeThreadId, (current) => ({ ...current, text: value }));
  }

  function selectChannel(channelId: string) {
    const nextChannel = data?.channels.find((item) => item.id === channelId) ?? null;
    setActiveChannelId(channelId);
    setShowMobileSidebar(false);
    setTaskDraft("");
    if (nextChannel?.kind === "dm") {
      setActiveTab("chat");
    }
    const repliedRootIds = new Set(
      visibleMessages
        .filter((message) => message.channel_id === channelId && message.thread_root_id)
        .map((message) => message.thread_root_id),
    );
    const first = visibleMessages.find((m) => m.channel_id === channelId && !m.thread_root_id && repliedRootIds.has(m.id));
    openThread(first?.id ?? null);
  }

  function openThread(threadId: string | null) {
    setActiveThreadId(threadId);
    setFocusedMessageId(null);
    if (!threadId) return;
    setThreadUnreadCounts((current) => {
      if (!current[threadId]) return current;
      const next = { ...current };
      delete next[threadId];
      return next;
    });
  }

  function revealThread(threadId: string | null) {
    openThread(threadId);
    if (threadId) setShowThread(true);
  }

  function navigateMobileBack(fallback: () => void) {
    if (isMobileViewport() && mobileHistoryReadyRef.current && mobileHistoryIndexRef.current > 0) {
      window.history.back();
      return;
    }
    fallback();
  }

  function closeMobileSidebar() {
    navigateMobileBack(() => setShowMobileSidebar(false));
  }

  function closeSelectedAgent() {
    navigateMobileBack(() => setSelectedAgentId(null));
  }

  function closeThreadPanel() {
    navigateMobileBack(() => {
      openThread(null);
      setShowThread(false);
    });
  }

  function addOptimisticOwnerMessage(channelId: string, threadRootId: string | null, body: string, asTask: boolean) {
    const id = `local-${clientId()}`;
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
      attachments: [],
      artifacts: [],
      created_at: new Date().toISOString(),
      updated_at: new Date().toISOString(),
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

  function appendDraftAttachments(files: FileList | File[], target: "root" | "reply") {
    const nextAttachments = Array.from(files)
      .filter((file) => {
        if (file.size <= MAX_ATTACHMENT_BYTES) return true;
        setAppError(`${file.name} is larger than 25MB`);
        return false;
      })
      .map(draftAttachmentFromFile);
    if (nextAttachments.length === 0) return;
    if (target === "root") {
      updateRootComposerDraft(activeChannelId, (current) => ({
        ...current,
        attachments: [...current.attachments, ...nextAttachments],
      }));
    } else {
      updateReplyComposerDraft(activeThreadId, (current) => ({
        ...current,
        attachments: [...current.attachments, ...nextAttachments],
      }));
    }
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
      workingDirectory: agentDraft.workingDirectory.trim() || defaultAgentWorkspace(handle),
    };
    const agentId = await apiInvoke<string>("create_agent", {
      handle,
      displayName: nextForm.displayName,
      role: nextForm.role,
      runtime: nextForm.runtime,
      model: nextForm.model,
      avatar: nextForm.avatar,
      description: nextForm.description,
      launchCommand: nextForm.launchCommand,
      workingDirectory: nextForm.workingDirectory,
      dailyBudgetMicros: budgetMicrosFromForm(nextForm.dailyBudgetUsd),
    });
    if (channel) {
      if (channel.kind !== "dm") {
        await apiInvoke("set_channel_agent_membership", {
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
      role: agent.role || "agent",
      avatar: agent.avatar || "",
      runtime: agent.runtime,
      model: agent.model,
      description: agent.description,
      launchCommand: agent.launch_command,
      workingDirectory: agent.working_directory,
      dailyBudgetUsd: budgetUsdFromMicros(agent.daily_budget_micros),
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
      workingDirectory: agentEdit.workingDirectory.trim(),
    };
    await mutate("update_agent", {
      agentId: editingAgentId,
      handle: nextForm.handle,
      displayName: nextForm.displayName || nextForm.handle,
      role: nextForm.role,
      runtime: nextForm.runtime,
      model: nextForm.model,
      avatar: nextForm.avatar,
      description: nextForm.description,
      launchCommand: nextForm.launchCommand,
      workingDirectory: nextForm.workingDirectory,
      dailyBudgetMicros: budgetMicrosFromForm(nextForm.dailyBudgetUsd),
    });
    setEditingAgentId(null);
    setAgentEdit(EMPTY_AGENT_FORM);
  }

  function cancelEditAgent() {
    setEditingAgentId(null);
    setAgentEdit(EMPTY_AGENT_FORM);
  }

  async function deleteAgent(agent: Agent) {
    const agentDm = data?.channels.find((item) => item.kind === "dm" && item.dm_agent_id === agent.id) ?? null;
    const fallbackChannelId = data?.channels.find((item) => item.id !== agentDm?.id)?.id ?? null;
    setConfirmRequest({
      title: `Delete @${agent.handle}?`,
      body: "This removes the agent profile, DM, schedules, runtime sessions, runs, and pending requests. Existing channel messages keep their sender name.",
      confirmLabel: "Delete agent",
      onConfirm: async () => {
        await mutate("delete_agent", { agentId: agent.id });
        if (editingAgentId === agent.id) setEditingAgentId(null);
        if (selectedAgentId === agent.id) setSelectedAgentId(null);
        if (agentDm && activeChannelId === agentDm.id) {
          setActiveChannelId(fallbackChannelId ?? "");
          setActiveThreadId(null);
        }
      },
    });
  }

  async function sendRootMessage(asTask = false) {
    if (!channel || (!draft.trim() && draftAttachments.length === 0)) return;
    const body = draft.trim();
    const attachments = draftAttachments;
    const sendAsTask = channel.kind === "dm" ? false : asTask;
    const optimisticId = attachments.length === 0
      ? addOptimisticOwnerMessage(channel.id, null, body, sendAsTask)
      : null;
    updateRootComposerDraft(channel.id, () => EMPTY_COMPOSER_DRAFT);
    try {
      await apiInvoke("send_message", {
        channelId: channel.id,
        threadRootId: null,
        body,
        asTask: sendAsTask,
        attachments: await attachmentUploads(attachments),
      });
      await refresh();
    } catch (err) {
      if (optimisticId) removeOptimisticMessage(optimisticId);
      updateRootComposerDraft(channel.id, () => ({ text: body, attachments }));
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
      attachments: [],
    });
    setTaskDraft("");
  }

  async function openDmWithAgent(agent: Agent) {
    try {
      const channelId = await apiInvoke<string>("open_dm_with_agent", { agentId: agent.id });
      await refresh();
      setActiveChannelId(channelId);
      setActiveThreadId(null);
      setActiveTab("chat");
      setTaskDraft("");
      setSelectedAgentId(null);
    } catch (err) {
      const message = errorMessage(err, "Failed to open direct message");
      setAppError(message);
      console.error(err);
    }
  }

  async function sendReply() {
    if (!channel || !activeRoot || (!replyDraft.trim() && replyAttachments.length === 0)) return;
    const body = replyDraft.trim();
    const attachments = replyAttachments;
    const optimisticId = attachments.length === 0
      ? addOptimisticOwnerMessage(channel.id, activeRoot.id, body, false)
      : null;
    updateReplyComposerDraft(activeRoot.id, () => EMPTY_COMPOSER_DRAFT);
    try {
      await apiInvoke("send_message", {
        channelId: channel.id,
        threadRootId: activeRoot.id,
        body,
        asTask: false,
        attachments: await attachmentUploads(attachments),
      });
      await refresh();
    } catch (err) {
      if (optimisticId) removeOptimisticMessage(optimisticId);
      updateReplyComposerDraft(activeRoot.id, () => ({ text: body, attachments }));
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
    revealThread(task.message_id);
    setActiveTab("chat");
  }

  function openWorkItem(item: AgentWorkItem) {
    if (item.channel_id) setActiveChannelId(item.channel_id);
    if (item.thread_root_id) {
      revealThread(item.thread_root_id);
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
      revealThread(result.threadId);
      setActiveTab("chat");
    }
    setShowSearchModal(false);
  }

  function openSavedMessage(item: SavedMessage) {
    selectChannel(item.channel_id);
    revealThread(item.thread_root_id ?? item.message_id);
    setFocusedMessageId(item.message_id);
    setActiveTab("chat");
    setShowSavedModal(false);
  }

  function openInboxItem(item: InboxItem) {
    if (item.channelId) selectChannel(item.channelId);
    if (item.threadId) {
      revealThread(item.threadId);
      setActiveTab("chat");
    }
    setShowInboxModal(false);
  }

  async function markInboxItemRead(item: InboxItem) {
    setDismissedInboxItems((current) => ({ ...current, [item.id]: item.timestamp }));
    if (item.threadId) {
      setThreadUnreadCounts((current) => {
        if (!current[item.threadId!]) return current;
        const next = { ...current };
        delete next[item.threadId!];
        return next;
      });
    }
    if (item.channelId) {
      setChannelAlertIds((current) => {
        if (!current.has(item.channelId!)) return current;
        const next = new Set(current);
        next.delete(item.channelId!);
        return next;
      });
      await apiInvoke("mark_channel_read", { channelId: item.channelId });
    }
    if (item.reminderId) {
      await apiInvoke("complete_reminder", { reminderId: item.reminderId });
    }
    await refresh();
  }

  async function markAllInboxRead() {
    if (!data) return;
    setDismissedInboxItems((current) => {
      const next = { ...current };
      for (const item of inboxItems) {
        next[item.id] = item.timestamp;
      }
      return next;
    });
    const channelIds = new Set<string>();
    for (const item of inboxItems) {
      if (item.channelId && (item.unread || channelAlertIds.has(item.channelId))) {
        channelIds.add(item.channelId);
      }
    }
    await Promise.all([...channelIds].map((channelId) => apiInvoke("mark_channel_read", { channelId })));
    await Promise.all(data.reminders
      .filter((reminder) => reminder.status === "fired")
      .map((reminder) => apiInvoke("complete_reminder", { reminderId: reminder.id })));
    setChannelAlertIds(new Set());
    setThreadUnreadCounts({});
    await refresh();
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

  function startAgentDrawerResize(event: ReactPointerEvent<HTMLButtonElement>) {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = agentDrawerWidth;

    function onPointerMove(moveEvent: PointerEvent) {
      const delta = startX - moveEvent.clientX;
      const maxWidth = Math.max(
        MIN_AGENT_DRAWER_WIDTH,
        Math.min(maxAgentDrawerWidth(), window.innerWidth - sidebarWidth - MIN_CONVERSATION_WIDTH),
      );
      const next = Math.min(maxWidth, Math.max(MIN_AGENT_DRAWER_WIDTH, startWidth + delta));
      setAgentDrawerWidth(next);
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

  async function setMessageSaved(message: Message, saved: boolean) {
    await mutate("set_message_saved", { messageId: message.id, saved });
  }

  async function unsaveSavedMessage(item: SavedMessage) {
    await mutate("set_message_saved", { messageId: item.message_id, saved: false });
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
      className={`app theme-liquid ${selectedAgent || showThread ? "" : "thread-hidden"} ${showMobileSidebar ? "mobile-sidebar-open" : ""}`}
      style={{
        "--sidebar-width": `${sidebarWidth}px`,
        "--thread-width": `${selectedAgent ? agentDrawerWidth : threadPanelWidth}px`,
      } as CSSProperties}
    >
      <Sidebar
        data={data}
        channel={channel}
        channelAlertIds={channelAlertIds}
        inboxUnreadCount={inboxUnreadCount}
        openSearch={() => {
          setShowMobileSidebar(false);
          setShowSearchModal(true);
        }}
        openInbox={() => {
          setShowMobileSidebar(false);
          setShowInboxModal(true);
        }}
        openSaved={() => {
          setShowMobileSidebar(false);
          setShowSavedModal(true);
        }}
        openCreateChannelModal={() => {
          setShowMobileSidebar(false);
          setShowCreateChannelModal(true);
        }}
        selectChannel={selectChannel}
        openCreateAgentModal={() => {
          setShowMobileSidebar(false);
          setShowCreateAgentModal(true);
        }}
        openDmWithAgent={(agent) => {
          setShowMobileSidebar(false);
          openDmWithAgent(agent);
        }}
        onMobileClose={closeMobileSidebar}
        onResizeStart={startSidebarResize}
      />
      <button
        type="button"
        className="mobile-sidebar-backdrop"
        aria-label="Close navigation"
        onClick={closeMobileSidebar}
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

      <InboxModal
        open={showInboxModal}
        items={inboxItems}
        onOpenItem={openInboxItem}
        onMarkItemRead={markInboxItemRead}
        onMarkAllRead={markAllInboxRead}
        onClose={() => setShowInboxModal(false)}
      />

      <SavedMessagesModal
        open={showSavedModal}
        items={data.saved_messages}
        onOpenItem={openSavedMessage}
        onUnsaveItem={unsaveSavedMessage}
        onClose={() => setShowSavedModal(false)}
      />

      <Conversation
        channel={channel}
        agents={data.agents}
        channelAgents={channelAgents}
        activeTab={activeTab}
        activeRoot={activeRoot}
        rootMessages={rootMessages}
        respondingAgents={channelRespondingAgents}
        threadReplyCounts={threadReplyCounts}
        visibleTasks={visibleTasks}
        draft={draft}
        draftAttachments={draftAttachments}
        taskDraft={taskDraft}
        taskTitleDrafts={taskTitleDrafts}
        setActiveTab={setActiveTab}
        setActiveThreadId={revealThread}
        openMobileSidebar={() => setShowMobileSidebar(true)}
        openChannelSettingsModal={() => setShowChannelSettingsModal(true)}
        deleteChannel={deleteChannel}
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
        addDraftAttachments={(files) => appendDraftAttachments(files, "root")}
        removeDraftAttachment={(id) => updateRootComposerDraft(activeChannelId, (current) => ({
          ...current,
          attachments: current.attachments.filter((item) => item.id !== id),
        }))}
        sendRootMessage={sendRootMessage}
        openAgentDetail={(agent) => setSelectedAgentId(agent.id)}
        openArtifact={openArtifact}
        shareBaseUrl={shareBaseUrl}
        savedMessageIds={savedMessageIds}
        focusedMessageId={focusedMessageId}
        onToggleMessageSaved={setMessageSaved}
      />

      {selectedAgent ? (
        <AgentDetailDrawer
          agent={selectedAgent}
          activeRun={selectedAgentRun}
          phase={selectedAgentPhase}
          activities={selectedAgentActivities}
          performance={selectedAgentPerformance}
          workItems={selectedAgentWorkItems}
          reminders={data.reminders}
          onClose={closeSelectedAgent}
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
          onResizeStart={startAgentDrawerResize}
        />
      ) : showThread && (
        <ThreadPanel
          channel={channel}
          agents={data.agents}
          activeRoot={activeRoot}
          activeTask={activeTask}
          replies={replies}
          respondingAgents={threadRespondingAgents}
          unreadCount={activeThreadId ? threadUnreadCounts[activeThreadId] ?? 0 : 0}
          taskTitleDrafts={taskTitleDrafts}
          replyDraft={replyDraft}
          replyAttachments={replyAttachments}
          onClose={closeThreadPanel}
          setTaskTitleDraft={setTaskTitleDraft}
          saveTaskTitle={saveTaskTitle}
          claimTask={claimTask}
          updateTaskStatus={updateTaskStatus}
          setReplyDraft={setReplyDraft}
          addReplyAttachments={(files) => appendDraftAttachments(files, "reply")}
          removeReplyAttachment={(id) => updateReplyComposerDraft(activeThreadId, (current) => ({
            ...current,
            attachments: current.attachments.filter((item) => item.id !== id),
          }))}
          sendReply={sendReply}
          openAgentDetail={(agent) => setSelectedAgentId(agent.id)}
          openArtifact={openArtifact}
          shareBaseUrl={shareBaseUrl}
          savedMessageIds={savedMessageIds}
          focusedMessageId={focusedMessageId}
          onToggleMessageSaved={setMessageSaved}
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

      <ConfirmModal
        open={Boolean(confirmRequest)}
        title={confirmRequest?.title ?? ""}
        body={confirmRequest?.body ?? ""}
        confirmLabel={confirmRequest?.confirmLabel ?? "Confirm"}
        onCancel={() => setConfirmRequest(null)}
        onConfirm={confirmRequest?.onConfirm ?? (() => {})}
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

createRoot(document.getElementById("root")!).render(
  <AppErrorBoundary>
    <App />
  </AppErrorBoundary>,
);
