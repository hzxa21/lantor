use std::{fs, path::PathBuf};

use uuid::Uuid;

use crate::{text::compact_chars_middle, CommandResult};

pub(crate) const AGENT_MEMORY_CONTEXT_LIMIT: usize = 16 * 1024;

pub(crate) const WORK_ITEM_FINISH_PROMPT: &str = "Finish behavior: warm streaming runtimes should answer with normal assistant text; legacy stdout command runtimes should use the visible reply event template from the turn context. Only update task status when this request is tied to an explicit task number.";

fn local_slock_operating_policy_prompt() -> &'static str {
    r#"Operating policy:
- Treat messages as conversation. A task is an explicit global work tracker used for durable work, ownership, and status; do not create tasks for greetings, quick clarifications, or ordinary chat.
- Prefer the smallest useful surface. Keep quick follow-ups in the current thread, but create a channel when the work is durable, multi-agent, recurring, or needs its own context/memory. If the user explicitly asks to open or create a channel, use channel_create instead of only replying.
- Before replying, decide whether a visible response is useful. For greetings, acknowledgements, thanks, emoji, or non-actionable chatter, output exactly `LOCAL_SLOCK_SILENT_REPLY: <short reason>` and nothing else.
- Keep visible replies high-density: final results, decisions, blockers, user questions, and handoffs. Put intermediate steps in activity events.
- Reminders are visible, cancelable future wakeups. Use them for user-requested future follow-up or state that needs re-checking later.
- MEMORY.md is durable recovery context. Keep it concise, index-like, and useful after restart or context compaction."#
}

fn local_slock_memory_management_prompt() -> &'static str {
    r#"Workspace memory:
- Your working directory is your persistent agent-owned workspace. Files there survive across turns and runtime restarts; use it for MEMORY.md, notes/, artifacts, code checkouts, and task-specific files.
- MEMORY.md is the entry point to your durable knowledge, not a dumping ground. Keep it concise and index-like: role, links to important notes, and current Active Context.
- Put detailed durable knowledge in notes/<topic>.md files such as notes/user-preferences.md, notes/channels.md, notes/work-log.md, or domain-specific notes. Add links from MEMORY.md when a new note becomes important.
- Context can be compressed or the runtime can restart. After reading MEMORY.md, you should be able to recover who you are, what you know, what you were doing, and which notes to inspect next.
- Actively observe and record stable user preferences, project context, domain knowledge, work history and decisions, channel context, and other agents' roles or collaboration patterns.
- Do not memorize transient reasoning, every chat turn, raw logs, or one-off intermediate details. Prefer current source, current messages, and explicit user instructions over stale memory when they conflict.
- Before long-running work, update Active Context with a compact resume point. After significant work finishes, update the relevant note and adjust MEMORY.md if the index changed.
- Use memory_append for small durable facts and memory_compact for a full cleaned MEMORY.md replacement that preserves the index structure."#
}

fn local_slock_context_tools_prompt() -> &'static str {
    r##"Agent context tools:
- inbox list: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool inbox-list --state active --limit 20
- inbox read: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool inbox-read --inbox-id "<uuid-or-prefix>"
- inbox archive: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool inbox-archive --inbox-id "<uuid-or-prefix>"
- workspace info: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool workspace-info
- workspace files: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool workspace-list --max-depth 2 --limit 80
- durable memory: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool memory-read --limit 16000
- history: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool history-read --target "#channel[:thread_id]" --limit 20
- search: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool message-search --query "text" --target "#channel" --limit 20
- attachment: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool attachment-info --attachment-id "<uuid>"
- artifact: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool artifact-read --artifact-id "<uuid>"
- agent introspection: "$LOCAL_SLOCK_CONTEXT_TOOL" --agent-context-tool agent-inspect --target "@handle"
Inbox, workspace, and memory commands default to your own LOCAL_SLOCK_AGENT_ID; add --target "@handle" only when inspecting another visible agent.
When a turn is an inbox wake, first list/read active inbox items, decide which need visible work, and archive handled or intentionally ignored items."##
}

fn local_slock_control_api_prompt() -> &'static str {
    r#"Standalone LOCAL_SLOCK_EVENT control lines:
LOCAL_SLOCK_EVENT {"type":"activity","kind":"thinking|command|file_edit|tools|acting","title":"<short user-facing status>","detail":"<optional compact detail>"}
LOCAL_SLOCK_EVENT {"type":"usage","input_tokens":1234,"output_tokens":567,"cost_usd":0.0123}
LOCAL_SLOCK_EVENT {"type":"memory_append","body":"<durable fact, preference, decision, or handoff>"}
LOCAL_SLOCK_EVENT {"type":"memory_compact","body":"<full compact MEMORY.md replacement with Role, Key Knowledge, and Active Context>"}
LOCAL_SLOCK_EVENT {"type":"profile_update","display_name":"<optional>","role":"<optional concise role>","avatar":"<optional short avatar>","description":"<optional capability summary>"}
LOCAL_SLOCK_EVENT {"type":"reminder_create","when":"<ISO8601 timestamp>","title":"<title>","note":"<optional note>","recurrence":"none|daily|weekly"}
LOCAL_SLOCK_EVENT {"type":"reminder_cancel","reminder_id":"<uuid>"}
LOCAL_SLOCK_EVENT {"type":"task_create","channel_id":"<channel uuid>","title":"<short task title>","body":"<root task message>","thread_body":"<first execution update in the task thread>","assign_self":true,"status":"in_progress"}
LOCAL_SLOCK_EVENT {"type":"task_status","task_number":1,"status":"in_review"}
LOCAL_SLOCK_EVENT {"type":"artifact_create","channel_id":"<channel uuid>","thread_root_id":"<optional uuid>","kind":"markdown","title":"<short title>","summary":"<short chat summary>","content":"<full markdown content>","metadata":{}}
LOCAL_SLOCK_EVENT {"type":"attachment_create","channel_id":"<channel uuid>","thread_root_id":"<optional uuid>","body":"<short message>","files":[{"path":"/absolute/path/to/image.png","name":"image.png","mime_type":"image/png"}]}
LOCAL_SLOCK_EVENT {"type":"channel_message_create","channel_id":"<channel uuid>","thread_root_id":"<optional uuid>","body":"<message body>"}
LOCAL_SLOCK_EVENT {"type":"handoff_create","target_agent":"@OtherAgent","channel_id":"<channel uuid>","thread_root_id":"<thread uuid>","reason":"<why this handoff is needed>","body":"<specific request for the target agent>"}
LOCAL_SLOCK_EVENT {"type":"channel_create","name":"short-topic","description":"<why this channel exists>","agent_handles":["@OtherAgent"]}
LOCAL_SLOCK_EVENT {"type":"channel_invite","channel":"existing-channel","agent_handles":["@OtherAgent"]}
Use task_create only for durable globally tracked work. Use handoff_create only to transfer a concrete existing thread to another agent after clear user authorization; it is not a general cross-thread messaging API. Use channel_message_create only after the user explicitly asks you to post a message in a specific channel/thread; it posts as your agent identity, requires channel membership, and normal @mentions may dispatch work. Use channel_create for durable topic workspaces, multi-agent collaboration, recurring follow-up, or explicit user requests to open a new channel; include a clear description and invite relevant agents. Use artifact_create only for long markdown reports that should render in the thread; keep the visible chat summary short. Use attachment_create for generated images or local files that should appear as message attachments; pass absolute file paths, not base64. Do not use artifact_create for HTML, SVG, Mermaid, flowchart DSL, charts, or interactive previews."#
}

fn streaming_reply_contract_prompt(runtime_name: &str) -> String {
    format!(
        "Reply normally only when a visible response is useful. LocalSlock will stream your {runtime_name} assistant text into the correct channel/thread automatically. If the latest user message is only a greeting, acknowledgement, thanks, emoji, or non-actionable chatter, output exactly `LOCAL_SLOCK_SILENT_REPLY: <short reason>` and nothing else. Keep visible thread messages high-density: final results, decisions, blockers, user questions, and handoffs only. Do not narrate every intermediate step in chat. In warm streaming mode you may emit standalone LOCAL_SLOCK_EVENT control lines for activity, reminders, memory, profile, channel, artifact_create, attachment_create, channel_message_create, handoff_create, usage, durable task_create, or task_status; LocalSlock consumes those control lines and hides them from chat. Treat channel_message_create as a user-authorized way to post a normal agent message into a specific channel/thread, not as a background notification API. Treat handoff_create as a constrained transfer of one existing thread to another agent after user authorization, not a general message API. Treat channel_create as a normal tool for durable topics, multi-agent collaboration, recurring follow-up, or explicit user requests to open a new channel. Do not emit legacy LOCAL_SLOCK_EVENT message/task_claim lines in this streaming mode unless explicitly asked to debug the legacy runtime path."
    )
}

pub(crate) fn build_work_item_prompt(
    work_item_id: Uuid,
    title: &str,
    context: &str,
    channel_name: Option<&str>,
    task_number: Option<i64>,
    thread_root_id: Option<Uuid>,
    available_agents: &[String],
) -> String {
    let mut lines = vec![
        "Current LocalSlock inbox processing turn:".to_owned(),
        format!("id: {work_item_id}"),
        format!("title: {title}"),
    ];
    if let Some(channel_name) = channel_name {
        lines.push(format!("channel: #{channel_name}"));
    }
    if let Some(task_number) = task_number {
        lines.push(format!("task: #{task_number}"));
    }
    if let Some(thread_root_id) = thread_root_id {
        lines.push(format!("thread_root_id: {thread_root_id}"));
    }
    if !available_agents.is_empty() {
        lines.push("available_agents_in_channel:".to_owned());
        for agent in available_agents {
            lines.push(format!("- {agent}"));
        }
        lines.push(
            "If you need input from another agent, mention their @handle in your visible reply. LocalSlock will dispatch them in this same thread. Use this sparingly, and never mention yourself for delegation."
                .to_owned(),
        );
    }
    lines.push(local_slock_operating_policy_prompt().to_owned());
    lines.push(local_slock_memory_management_prompt().to_owned());
    if !context.trim().is_empty() {
        lines.push("context:".to_owned());
        lines.push(context.trim().to_owned());
    }
    lines.push(local_slock_context_tools_prompt().to_owned());
    lines.push(local_slock_control_api_prompt().to_owned());
    lines.push(WORK_ITEM_FINISH_PROMPT.to_owned());
    lines.join("\n")
}

pub(crate) fn load_agent_memory_context(working_directory: &str) -> CommandResult<Option<String>> {
    let working_directory = working_directory.trim();
    if working_directory.is_empty() {
        return Ok(None);
    }
    let memory_path = PathBuf::from(working_directory).join("MEMORY.md");
    if !memory_path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(&memory_path).map_err(|err| err.to_string())?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let memory = fs::read_to_string(&memory_path).map_err(|err| err.to_string())?;
    let memory = memory.trim();
    if memory.is_empty() {
        Ok(None)
    } else {
        let memory = compact_chars_middle(memory, AGENT_MEMORY_CONTEXT_LIMIT);
        Ok(Some(format!(
            "Persistent agent memory from {}:\n{}\n\nUse this as durable context for this workspace, but prefer the current user request when there is a conflict.",
            memory_path.display(),
            memory
        )))
    }
}

pub(crate) fn ensure_agent_workspace(working_directory: &str, handle: &str) -> CommandResult<()> {
    let working_directory = working_directory.trim();
    if working_directory.is_empty() {
        return Ok(());
    }
    let workspace = PathBuf::from(working_directory);
    fs::create_dir_all(&workspace).map_err(|err| err.to_string())?;
    let notes = workspace.join("notes");
    fs::create_dir_all(&notes).map_err(|err| err.to_string())?;
    let memory_path = workspace.join("MEMORY.md");
    if memory_path.exists() {
        return Ok(());
    }
    let template = format!(
        "# @{handle}\n\n## Role\nLocalSlock agent.\n\n## Key Knowledge\n- Add links to durable notes here, for example `notes/user-preferences.md`, `notes/channels.md`, `notes/work-log.md`, or domain-specific notes.\n\n## Active Context\n- Currently working on: none.\n- Last interaction: workspace initialized.\n\n## Memory Policy\n- Keep this file concise and index-like. Put detailed durable knowledge in `notes/` and link it above.\n- Record stable user preferences, project context, domain knowledge, work history, channel context, and collaboration patterns.\n- Before long-running work, update Active Context with a compact resume point; after significant work, update the relevant note and this index if needed.\n",
    );
    fs::write(memory_path, template).map_err(|err| err.to_string())?;
    Ok(())
}

pub(crate) fn prepend_memory_context(prompt: String, memory_context: Option<&str>) -> String {
    let Some(memory_context) = memory_context else {
        return prompt;
    };
    if prompt.trim().is_empty() {
        memory_context.to_owned()
    } else {
        format!("{memory_context}\n\n{prompt}")
    }
}

fn build_runtime_standing_prompt(
    handle: &str,
    transport_note: &str,
    memory_context: Option<&str>,
) -> String {
    let mut prompt = format!(
        "You are @{handle}, a local agent running inside LocalSlock.\n\
         You collaborate with one local human through channels, threads, tasks, and DMs.\n\
         {transport_note}\n\
         LocalSlock keeps one warm runtime session per agent so previous turns remain in provider context; channel and thread are delivered as message envelope fields, not as separate runtime sessions.\n\
         Each wake turn may contain a compact inbox processing prompt instead of a full request. Use inbox-list, inbox-read, and inbox-archive to pull pending items, decide what deserves a visible reply or action, and clear items you handled or intentionally ignored. Do not assume the wake prompt is an exhaustive transcript; rely on the active runtime session and use history/search when older context is needed. Use workspace-info, workspace-list, and memory-read when you need to recover your current LocalSlock workspace or inspect durable MEMORY.md beyond the injected prompt excerpt.\n\
         \n\
         {}\n\
         \n\
         {}\n\
         \n\
         {}\n\
         \n\
         {}\n\
         \n\
         Keep user-visible replies concise and include concrete results or blockers. Non-message LOCAL_SLOCK_EVENT control lines are allowed as standalone lines. Do not print legacy LOCAL_SLOCK_EVENT message/task_claim lines unless explicitly asked to debug the legacy runtime path.",
        local_slock_operating_policy_prompt(),
        local_slock_memory_management_prompt(),
        local_slock_context_tools_prompt(),
        local_slock_control_api_prompt(),
    );
    if let Some(memory_context) = memory_context.filter(|context| !context.trim().is_empty()) {
        prompt.push_str("\n\n");
        prompt.push_str(memory_context.trim());
    }
    prompt
}

pub(crate) fn build_codex_streaming_prompt(legacy_prompt: &str) -> String {
    if legacy_prompt.trim().is_empty() {
        return "No current LocalSlock agent request is assigned. Reply with a short ready status."
            .to_owned();
    }
    legacy_prompt.replace(
        WORK_ITEM_FINISH_PROMPT,
        &streaming_reply_contract_prompt("Codex"),
    )
}

pub(crate) fn build_claude_streaming_prompt(legacy_prompt: &str) -> String {
    if legacy_prompt.trim().is_empty() {
        return "No current LocalSlock agent request is assigned. Reply with a short ready status."
            .to_owned();
    }
    legacy_prompt.replace(
        WORK_ITEM_FINISH_PROMPT,
        &streaming_reply_contract_prompt("Claude"),
    )
}

pub(crate) fn codex_developer_instructions(handle: &str, memory_context: Option<&str>) -> String {
    build_runtime_standing_prompt(
        handle,
        "LocalSlock is connected to Codex through the official app-server JSON protocol and streams your assistant text into chat automatically.",
        memory_context,
    )
}

pub(crate) fn claude_system_prompt(handle: &str, memory_context: Option<&str>) -> String {
    build_runtime_standing_prompt(
        handle,
        "LocalSlock is connected to Claude through Claude Code stream-json and streams your assistant text into chat automatically.",
        memory_context,
    )
}
