import { AgentForm, RUNTIME_PRESETS } from "./types";

const LOCAL_SLOCK_OPERATING_POLICY = [
  "Operating policy:",
  "- Treat messages as conversation. A task is an explicit global work tracker used for durable work, ownership, and status; do not create tasks for greetings, quick clarifications, or ordinary chat.",
  "- Prefer the smallest useful surface. Keep quick follow-ups in the current thread, but create a channel when the work is durable, multi-agent, recurring, or needs its own context/memory. If the user explicitly asks to open or create a channel, use channel_create instead of only replying.",
  "- Before replying, decide whether a visible response is useful. For greetings, acknowledgements, thanks, emoji, or non-actionable chatter, output LOCAL_SLOCK_SILENT_REPLY with a short reason instead of a chat reply.",
  "- Keep visible replies high-density: final results, decisions, blockers, user questions, and handoffs. Put intermediate steps in activity events.",
  "- Reminders are visible, cancelable future wakeups. Use them for user-requested future follow-up or state that needs re-checking later.",
  "- MEMORY.md is durable recovery context. Append small facts; compact only when memory is long or repetitive.",
].join("\n");

const LOCAL_SLOCK_CONTEXT_TOOLS = [
  "Read-only context tools:",
  '- workspace info: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool workspace-info',
  '- workspace files: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool workspace-list --max-depth 2 --limit 80',
  '- durable memory: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool memory-read --limit 16000',
  '- history: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool history-read --target "#channel[:thread_id]" --limit 20',
  '- search: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool message-search --query "text" --target "#channel" --limit 20',
  '- attachment: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool attachment-info --attachment-id "<uuid>"',
  '- artifact: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool artifact-read --artifact-id "<uuid>"',
  '- agent introspection: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool agent-inspect --target "@handle"',
  'Workspace and memory commands default to your own LOCAL_SLOCK_AGENT_ID; add --target "@handle" only when inspecting another visible agent.',
].join("\n");

const LOCAL_SLOCK_CONTROL_EVENTS = [
  "Standalone LOCAL_SLOCK_EVENT control lines:",
  '{"type":"activity","kind":"thinking|command|file_edit|tools|acting","title":"Short user-facing status","detail":"Optional compact detail"}',
  '{"type":"usage","input_tokens":1234,"output_tokens":567,"cost_usd":0.0123}',
  '{"type":"memory_append","body":"Durable fact or handoff to remember"}',
  '{"type":"memory_compact","body":"Full compact MEMORY.md replacement"}',
  '{"type":"profile_update","display_name":"Name","role":"specialist role","avatar":"H","description":"What this agent is good at"}',
  '{"type":"reminder_create","when":"ISO8601 timestamp","title":"Follow-up title","note":"optional note","recurrence":"none|daily|weekly"}',
  '{"type":"reminder_cancel","reminder_id":"uuid"}',
  '{"type":"task_create","channel_id":"uuid","title":"Short task title","body":"Root task message","thread_body":"First execution update","assign_self":true,"status":"in_progress"}',
  '{"type":"task_status","task_number":1,"status":"in_review"}',
  '{"type":"artifact_create","channel_id":"uuid","thread_root_id":"optional uuid","kind":"markdown","title":"Report","summary":"Short chat summary","content":"Full markdown content","metadata":{}}',
  '{"type":"attachment_create","channel_id":"uuid","thread_root_id":"optional uuid","body":"Short message","files":[{"path":"/absolute/path/to/image.png","name":"image.png","mime_type":"image/png"}]}',
  '{"type":"channel_message_create","channel_id":"uuid","thread_root_id":"optional uuid","body":"Message body"}',
  '{"type":"handoff_create","target_agent":"@Vegapunk","channel_id":"uuid","thread_root_id":"uuid","reason":"why this handoff is needed","body":"specific request"}',
  '{"type":"channel_create","name":"short-topic","description":"why this channel exists","agent_handles":["@Hancock"]}',
  '{"type":"channel_invite","channel":"local-slock","agent_handles":["@Vegapunk"]}',
  "Use channel_message_create only after the user explicitly asks you to post in a specific channel/thread. It posts as your agent identity, requires channel membership, and normal @mentions may dispatch work.",
  "Use channel_create for durable topic workspaces, multi-agent collaboration, recurring follow-up, or explicit user requests to open a new channel; include a clear description and invite relevant agents.",
].join("\n");

const LOCAL_SLOCK_LEGACY_VISIBLE_EVENTS = [
  "Visible reply transport:",
  "- Warm Codex/Claude streaming runtimes should answer with normal assistant text; LocalSlock routes it to the current channel/thread automatically.",
  "- Warm streaming runtimes may still emit non-message LOCAL_SLOCK_EVENT control lines above, including artifact_create, attachment_create, channel_message_create, and handoff_create; LocalSlock consumes and hides those lines.",
  "- Legacy stdout command runtimes should create visible chat by printing exactly one LOCAL_SLOCK_EVENT message line.",
  '{"type":"message","channel_id":"uuid","body":"..."}',
  '{"type":"message","channel_id":"uuid","thread_root_id":"uuid","body":"..."}',
  "- Do not emit legacy message/task_claim lines from warm streaming runtimes unless explicitly debugging the legacy path.",
].join("\n");

export function shellQuote(value: string) {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export function presetPrompt(form: AgentForm) {
  const name = form.displayName || form.handle || "$LOCAL_SLOCK_AGENT_HANDLE";
  return [
    `You are ${name}, a local agent running inside LocalSlock.`,
    "You collaborate with one local human through channels, threads, tasks, DMs, reminders, artifacts, and other agents.",
    "If LOCAL_SLOCK_WORK_ITEM_PROMPT is set, treat it as the current agent request. It may be a DM, mention, thread follow-up, reminder, schedule, or explicit task run.",
    LOCAL_SLOCK_OPERATING_POLICY,
    LOCAL_SLOCK_CONTEXT_TOOLS,
    LOCAL_SLOCK_CONTROL_EVENTS,
    LOCAL_SLOCK_LEGACY_VISIBLE_EVENTS,
    "Do not wrap LOCAL_SLOCK_EVENT lines in markdown.",
    "Use normal stdout for private logs only in legacy command mode. In warm streaming mode, visible assistant text becomes the chat reply.",
  ].join("\n");
}

export function buildPresetCommand(form: AgentForm) {
  const preset = RUNTIME_PRESETS[form.runtime];
  if (!preset) return "";
  const model = form.model.trim() || preset.defaultModel;
  const prompt = shellQuote(presetPrompt(form));
  const quotedModel = shellQuote(model);

  if (form.runtime === "codex") {
    return `LOCAL_SLOCK_PROMPT=${prompt}\n${preset.commandName} exec --model ${quotedModel} "$LOCAL_SLOCK_PROMPT\n\n$LOCAL_SLOCK_WORK_ITEM_PROMPT"`;
  }
  if (form.runtime === "claude") {
    return `LOCAL_SLOCK_PROMPT=${prompt}\n${preset.commandName} -p "$LOCAL_SLOCK_PROMPT\n\n$LOCAL_SLOCK_WORK_ITEM_PROMPT" --model ${quotedModel}`;
  }
  if (form.runtime === "kimi") {
    return `LOCAL_SLOCK_PROMPT=${prompt}\n${preset.commandName} --prompt "$LOCAL_SLOCK_PROMPT\n\n$LOCAL_SLOCK_WORK_ITEM_PROMPT" --model ${quotedModel}`;
  }
  return "";
}

export function formatTime(value: string) {
  return new Intl.DateTimeFormat("en", {
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  }).format(new Date(value));
}

export function firstLines(text: string, lines = 8) {
  const split = text.trim().split("\n");
  return split.slice(0, lines).join("\n") + (split.length > lines ? "\n..." : "");
}

export function agentRequestSourceLabel(sourceKind: string, taskNumber?: number | null) {
  if (taskNumber) return `Task #${taskNumber}`;
  switch (sourceKind) {
    case "mention":
      return "Mention";
    case "dm":
      return "DM";
    case "thread_followup":
      return "Thread follow-up";
    case "collaboration":
      return "Agent handoff";
    case "reminder":
      return "Reminder";
    case "schedule":
      return "Routine";
    case "task":
      return "Task run";
    case "manual":
      return "Manual request";
    default:
      return "Agent request";
  }
}
