export type Agent = {
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

export type Channel = {
  id: string;
  name: string;
  description: string;
  kind: "channel" | "dm";
  dm_agent_id: string | null;
  unread_count: number;
};

export type ChannelMember = {
  channel_id: string;
  agent_id: string;
  agent_handle: string;
  agent_display_name: string;
  created_at: string;
};

export type Message = {
  id: string;
  channel_id: string;
  thread_root_id: string | null;
  sender_name: string;
  sender_role: string;
  body: string;
  is_task: boolean;
  thread_followed: boolean;
  delivery_state: "complete" | "streaming" | "error" | string;
  stream_key: string;
  task_number: number | null;
  task_status: string | null;
  created_at: string;
};

export type Task = {
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

export type AgentRun = {
  id: string;
  agent_id: string;
  agent_handle: string;
  work_item_id: string | null;
  command: string;
  working_directory: string;
  status: string;
  pid: number | null;
  exit_code: number | null;
  log: string;
  started_at: string;
  stopped_at: string | null;
};

export type AgentWorkItem = {
  id: string;
  agent_id: string;
  agent_handle: string;
  channel_id: string | null;
  channel_name: string | null;
  thread_root_id: string | null;
  source_message_id: string | null;
  task_id: string | null;
  task_number: number | null;
  title: string;
  context: string;
  status: string;
  run_id: string | null;
  created_at: string;
  updated_at: string;
  completed_at: string | null;
};

export type AgentActivity = {
  id: string;
  agent_id: string | null;
  agent_handle: string;
  run_id: string | null;
  kind: string;
  title: string;
  detail: string;
  created_at: string;
};

export type SupervisorStatus = {
  pid: number | null;
  status: string;
  updated_at: string | null;
};

export type LaunchAgentStatus = {
  label: string;
  plist_path: string;
  installed: boolean;
  loaded: boolean;
};

export type RuntimeCheck = {
  runtime: string;
  command: string;
  available: boolean;
  detail: string;
};

export type Bootstrap = {
  db_url: string;
  channels: Channel[];
  channel_members: ChannelMember[];
  agents: Agent[];
  messages: Message[];
  tasks: Task[];
  agent_runs: AgentRun[];
  agent_work_items: AgentWorkItem[];
  agent_activities: AgentActivity[];
  supervisor: SupervisorStatus;
  launch_agent: LaunchAgentStatus;
};

export type SearchScope = "all" | "messages" | "channels" | "tasks" | "agents" | "activity";

export type SearchTimeRange = "any" | "today" | "7d" | "30d";

export type SearchResult = {
  id: string;
  kind: string;
  title: string;
  detail: string;
  excerpt: string;
  createdAt: string | null;
  channelId: string | null;
  threadId: string | null;
  agentId: string | null;
};

export type AgentForm = {
  handle: string;
  displayName: string;
  runtime: string;
  model: string;
  description: string;
  launchCommand: string;
  workingDirectory: string;
};

export const EMPTY_AGENT_FORM: AgentForm = {
  handle: "",
  displayName: "",
  runtime: "codex",
  model: "gpt-5.5",
  description: "",
  launchCommand: "",
  workingDirectory: "",
};

export const TASK_STATUSES = ["todo", "in_progress", "in_review", "done"] as const;

export const ACTIVE_RUN_STATUSES = new Set(["starting", "running", "stopping"]);

export const RUNTIME_PRESETS: Record<string, { label: string; defaultModel: string; commandName: string; models: string[] }> = {
  codex: {
    label: "Codex",
    defaultModel: "gpt-5.5",
    commandName: "codex",
    models: ["gpt-5.5", "gpt-5.4", "gpt-5.4-mini", "gpt-5.3-codex"],
  },
  claude: {
    label: "Claude",
    defaultModel: "sonnet",
    commandName: "claude",
    models: ["sonnet", "opus", "haiku"],
  },
  kimi: {
    label: "Kimi",
    defaultModel: "kimi-k2",
    commandName: "kimi",
    models: ["kimi-k2", "kimi-k2-turbo"],
  },
};

export function modelOptionsForRuntime(runtime: string, currentModel = "") {
  const models = RUNTIME_PRESETS[runtime]?.models ?? [];
  if (!currentModel || models.includes(currentModel)) return models;
  return [currentModel, ...models];
}
