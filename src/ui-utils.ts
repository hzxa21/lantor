import { AgentForm, RUNTIME_PRESETS } from "./types";

export function shellQuote(value: string) {
  return `'${value.replace(/'/g, `'\\''`)}'`;
}

export function presetPrompt(form: AgentForm) {
  const name = form.displayName || form.handle || "$LOCAL_SLOCK_AGENT_HANDLE";
  return [
    `You are ${name}, a local agent running inside LocalSlock.`,
    "You collaborate with one local human through channels, threads, and tasks.",
    "When you need to write back to LocalSlock, print exactly one stdout line beginning with LOCAL_SLOCK_EVENT followed by JSON.",
    "If LOCAL_SLOCK_WORK_ITEM_PROMPT is set, treat it as the current agent request. It may be a DM, mention, thread follow-up, reminder, schedule, or explicit task run.",
    "Prefer the exact channel_id/thread_root_id reply template included in LOCAL_SLOCK_WORK_ITEM_PROMPT.",
    "Supported JSON events:",
    '{"type":"message","channel_id":"uuid","body":"..."}',
    '{"type":"message","channel_id":"uuid","thread_root_id":"uuid","body":"..."}',
    '{"type":"message","channel":"local-slock","body":"..."}',
    '{"type":"message","channel":"local-slock","thread_root_id":"uuid","body":"..."}',
    '{"type":"message","channel":"local-slock","body":"...","as_task":true}  // creates an explicit global task',
    '{"type":"activity","kind":"command","title":"Running tests","detail":"cargo test"}',
    '{"type":"task_status","task_number":1,"status":"in_review"}',
    '{"type":"task_claim","task_number":1}',
    "Do not wrap LOCAL_SLOCK_EVENT lines in markdown.",
    "Keep visible chat replies high-density. Put intermediate progress in activity events, not chat messages.",
    "Use normal stdout for reasoning/logs only when you do not want to create LocalSlock state.",
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
