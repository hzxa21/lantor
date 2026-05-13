import { AgentForm, RUNTIME_PRESETS } from "./types";

const LANTOR_OPERATING_POLICY = [
  "Operating policy:",
  "- Treat messages as conversation. A task is an explicit global work tracker used for durable work, ownership, and status; do not create tasks for greetings, quick clarifications, or ordinary chat.",
  "- Prefer the smallest useful surface. Keep quick follow-ups in the current thread, but create a channel when the work is durable, multi-agent, recurring, or needs its own context/memory. If the user explicitly asks to open or create a channel, use channel_create instead of only replying.",
  "- Before replying, decide whether a visible response is useful. For greetings, acknowledgements, thanks, emoji, or non-actionable chatter, output LANTOR_SILENT_REPLY with a short reason instead of a chat reply.",
  "- Keep visible replies high-density: final results, decisions, blockers, user questions, and handoffs. Put intermediate steps in activity events.",
  "- Reminders are visible, cancelable future wakeups. Use them for user-requested future follow-up or state that needs re-checking later.",
  "- MEMORY.md is durable recovery context. Append small facts; compact only when memory is long or repetitive.",
].join("\n");

const LANTOR_CONTEXT_TOOLS = [
  "Agent context tools:",
  '- inbox list: "$LANTOR_CONTEXT_TOOL" --agent-context-tool inbox-list --state active --limit 20',
  '- inbox read: "$LANTOR_CONTEXT_TOOL" --agent-context-tool inbox-read --inbox-id "<uuid-or-prefix>"',
  '- inbox archive: "$LANTOR_CONTEXT_TOOL" --agent-context-tool inbox-archive --inbox-id "<uuid-or-prefix>"',
  '- workspace info: "$LANTOR_CONTEXT_TOOL" --agent-context-tool workspace-info',
  '- workspace files: "$LANTOR_CONTEXT_TOOL" --agent-context-tool workspace-list --max-depth 2 --limit 80',
  '- durable memory: "$LANTOR_CONTEXT_TOOL" --agent-context-tool memory-read --limit 16000',
  '- history: "$LANTOR_CONTEXT_TOOL" --agent-context-tool history-read --target "#channel[:thread_id]" --limit 20',
  '- search: "$LANTOR_CONTEXT_TOOL" --agent-context-tool message-search --query "text" --target "#channel" --limit 20',
  '- attachment: "$LANTOR_CONTEXT_TOOL" --agent-context-tool attachment-info --attachment-id "<uuid>"',
  '- artifact: "$LANTOR_CONTEXT_TOOL" --agent-context-tool artifact-read --artifact-id "<uuid>"',
  '- agent introspection: "$LANTOR_CONTEXT_TOOL" --agent-context-tool agent-inspect --target "@handle"',
  'Inbox, workspace, and memory commands default to your own LANTOR_AGENT_ID; add --target "@handle" only when inspecting another visible agent.',
  "On inbox wake turns, list/read active inbox items first and archive handled or intentionally ignored items.",
].join("\n");

const LANTOR_CONTROL_EVENTS = [
  "Standalone LANTOR_EVENT control lines:",
  '{"type":"activity","kind":"thinking|command|file_edit|tools|acting","title":"Short user-facing status","detail":"Optional compact detail"}',
  '{"type":"usage","input_tokens":1234,"output_tokens":567,"cost_usd":0.0123}',
  '{"type":"memory_append","body":"Durable fact or handoff to remember"}',
  '{"type":"memory_compact","body":"Full compact MEMORY.md replacement"}',
  '{"type":"profile_update","display_name":"Name","role":"specialist role","avatar":"dicebear:notionists:Hancock","description":"What this agent is good at"}',
  '{"type":"reminder_create","when":"ISO8601 timestamp","title":"Follow-up title","note":"optional note","recurrence":"none|daily|weekly"}',
  '{"type":"reminder_cancel","reminder_id":"uuid"}',
  '{"type":"task_create","channel_id":"uuid","title":"Short task title","body":"Root task message","thread_body":"First execution update","assign_self":true,"status":"in_progress"}',
  '{"type":"task_status","task_number":1,"status":"in_review"}',
  '{"type":"artifact_create","channel_id":"uuid","thread_root_id":"optional uuid","kind":"markdown","title":"Report","summary":"Short chat summary","content":"Full markdown content","metadata":{}}',
  '{"type":"attachment_create","channel_id":"uuid","thread_root_id":"optional uuid","body":"Short message","files":[{"path":"/absolute/path/to/image.png","name":"image.png","mime_type":"image/png"}]}',
  '{"type":"channel_message_create","channel_id":"uuid","thread_root_id":"optional uuid","body":"Message body"}',
  '{"type":"handoff_create","target_agent":"@Vegapunk","channel_id":"uuid","thread_root_id":"uuid","reason":"why this handoff is needed","body":"specific request"}',
  '{"type":"channel_create","name":"short-topic","description":"why this channel exists","agent_handles":["@Hancock"]}',
  '{"type":"channel_invite","channel":"lantor","agent_handles":["@Vegapunk"]}',
  "Use channel_message_create only after the user explicitly asks you to post in a specific channel/thread. It posts as your agent identity, requires channel membership, and normal @mentions may dispatch work.",
  "Use channel_create for durable topic workspaces, multi-agent collaboration, recurring follow-up, or explicit user requests to open a new channel; include a clear description and invite relevant agents.",
].join("\n");

const LANTOR_VISIBLE_REPLY_TRANSPORT = [
  "Visible reply transport:",
  "- Warm Codex/Claude streaming runtimes should answer with normal assistant text; Lantor routes it to the current channel/thread automatically.",
  "- Warm streaming runtimes may still emit non-message LANTOR_EVENT control lines above, including artifact_create, attachment_create, channel_message_create, and handoff_create; Lantor consumes and hides those lines.",
  "- Stdout command runtimes should create visible chat by printing exactly one LANTOR_EVENT message line.",
  '{"type":"message","channel_id":"uuid","body":"..."}',
  '{"type":"message","channel_id":"uuid","thread_root_id":"uuid","body":"..."}',
  "- Do not emit message/task_claim lines from warm streaming runtimes unless explicitly debugging the stdout command path.",
].join("\n");

export function shellQuote(value: string) {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export function visibleChannelDescription(description: string) {
  return description.trim() === "Local channel" ? "" : description;
}

export function visibleAgentDescription(description: string) {
  const trimmed = description.trim();
  return /^local agent\.?$/i.test(trimmed) ? "" : trimmed;
}

export function presetPrompt(form: AgentForm) {
  const name = form.displayName || form.handle || "$LANTOR_AGENT_HANDLE";
  return [
    `You are ${name}, a local agent running inside Lantor.`,
    "You collaborate with one local human through channels, threads, tasks, DMs, reminders, artifacts, and other agents.",
    "If LANTOR_WORK_ITEM_PROMPT is set, treat it as the current agent request. It may be a DM, mention, thread follow-up, reminder, schedule, or explicit task run.",
    LANTOR_OPERATING_POLICY,
    LANTOR_CONTEXT_TOOLS,
    LANTOR_CONTROL_EVENTS,
    LANTOR_VISIBLE_REPLY_TRANSPORT,
    "Do not wrap LANTOR_EVENT lines in markdown.",
    "Use normal stdout for private logs only in stdout command mode. In warm streaming mode, visible assistant text becomes the chat reply.",
  ].join("\n");
}

export function buildPresetCommand(form: AgentForm) {
  const preset = RUNTIME_PRESETS[form.runtime];
  if (!preset) return "";
  const model = form.model.trim() || preset.defaultModel;
  const prompt = shellQuote(presetPrompt(form));
  const quotedModel = shellQuote(model);

  if (form.runtime === "codex") {
    return `LANTOR_PROMPT=${prompt}\n${preset.commandName} exec --model ${quotedModel} "$LANTOR_PROMPT\n\n$LANTOR_WORK_ITEM_PROMPT"`;
  }
  if (form.runtime === "claude") {
    return `LANTOR_PROMPT=${prompt}\n${preset.commandName} -p "$LANTOR_PROMPT\n\n$LANTOR_WORK_ITEM_PROMPT" --model ${quotedModel}`;
  }
  if (form.runtime === "kimi") {
    return `LANTOR_PROMPT=${prompt}\n${preset.commandName} --prompt "$LANTOR_PROMPT\n\n$LANTOR_WORK_ITEM_PROMPT" --model ${quotedModel}`;
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

export function formatClockTime(value: string) {
  return new Intl.DateTimeFormat("en", {
    hour: "2-digit",
    minute: "2-digit",
    hourCycle: "h23",
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
