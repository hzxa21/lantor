#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::{HashMap, HashSet},
    env, fs,
    path::PathBuf,
    process::{Command as StdCommand, Stdio},
    sync::Arc,
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::{
    postgres::{PgListener, PgPoolOptions},
    PgPool, Row,
};
use tauri::{Emitter, Manager, State};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, AsyncWriteExt, BufReader},
    process::Command,
    sync::Mutex as AsyncMutex,
    time::sleep,
};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://dylan:123456@127.0.0.1:5432/localslock";
const SUPERVISOR_LOCK_ID: i64 = 2_026_050_101;
const LAUNCH_AGENT_LABEL: &str = "local.localslock.supervisor";
const AGENT_EVENT_PREFIX: &str = "LOCAL_SLOCK_EVENT ";
const SILENT_REPLY_PREFIX: &str = "LOCAL_SLOCK_SILENT_REPLY";
const UI_REFRESH_CHANNEL: &str = "localslock_ui_refresh";
const SUPERVISOR_WAKE_CHANNEL: &str = "localslock_supervisor_wake";
const UI_REFRESH_EVENT: &str = "localslock://refresh";
const LOCAL_SLOCK_CONTEXT_TOOL_ENV: &str = "LOCAL_SLOCK_CONTEXT_TOOL";
const STREAMING_MESSAGE_BODY_LIMIT: usize = 200_000;
const STREAMING_TRUNCATION_MARKER: &str = "\n\n[stream truncated by LocalSlock]";
const ATTACHMENT_SIZE_LIMIT: usize = 25 * 1024 * 1024;
const AGENT_MEMORY_CONTEXT_LIMIT: usize = 16 * 1024;
const DISPATCH_CONTEXT_LIMIT: usize = 18 * 1024;
const DISPATCH_MESSAGE_BODY_LIMIT: usize = 4 * 1024;
const DISPATCH_THREAD_MESSAGE_LIMIT: i64 = 6;
const DISPATCH_THREAD_MESSAGE_BODY_LIMIT: usize = 1_500;
const AGENT_CONTEXT_TOOL_MESSAGE_LIMIT: usize = 2_000;
const CODEX_IDLE_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const CODEX_IDLE_REAPER_INTERVAL: Duration = Duration::from_secs(30);

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    db_url: String,
}

#[derive(Clone, Default)]
struct WarmCodexRegistry {
    runtimes: Arc<AsyncMutex<HashMap<Uuid, Arc<WarmCodexRuntime>>>>,
}

#[derive(Clone, Default)]
struct WarmClaudeRegistry {
    runtimes: Arc<AsyncMutex<HashMap<Uuid, Arc<WarmClaudeRuntime>>>>,
}

struct WarmCodexRuntime {
    stdin: AsyncMutex<tokio::process::ChildStdin>,
    state: AsyncMutex<WarmCodexState>,
    thread_id: String,
    pid: Option<i32>,
}

struct WarmCodexState {
    alive: bool,
    active: Option<CodexActiveTurn>,
    next_request_id: i64,
    last_activity: Instant,
}

struct CodexActiveTurn {
    run_id: Uuid,
    turn_request_id: i64,
    turn_id: Option<String>,
    started_at: Instant,
    first_delta_at: Option<Instant>,
    work_item_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
    stream_keys: HashSet<String>,
    steer_requests: HashMap<i64, CodexSteerRequest>,
    steer_disabled: bool,
    interrupt_request_id: Option<i64>,
}

struct CodexSteerRequest {
    work_item_id: Uuid,
    run_id: Uuid,
}

struct WarmClaudeRuntime {
    stdin: AsyncMutex<tokio::process::ChildStdin>,
    state: AsyncMutex<WarmClaudeState>,
    pid: Option<i32>,
}

struct WarmClaudeState {
    alive: bool,
    active: Option<ClaudeActiveTurn>,
    session_id: Option<String>,
    last_activity: Instant,
}

struct ClaudeActiveTurn {
    run_id: Uuid,
    started_at: Instant,
    first_delta_at: Option<Instant>,
    work_item_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
    stream_key: String,
}

#[derive(Debug, Serialize)]
struct RuntimeCheck {
    runtime: String,
    command: String,
    available: bool,
    detail: String,
}

#[derive(Debug, Serialize)]
struct Agent {
    id: Uuid,
    handle: String,
    display_name: String,
    role: String,
    status: String,
    runtime: String,
    model: String,
    avatar: String,
    description: String,
    launch_command: String,
    working_directory: String,
    daily_budget_micros: i64,
}

#[derive(Debug, Serialize)]
struct Channel {
    id: Uuid,
    name: String,
    description: String,
    kind: String,
    dm_agent_id: Option<Uuid>,
    unread_count: i32,
}

#[derive(Debug, Serialize)]
struct ChannelMember {
    channel_id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
    agent_display_name: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct Message {
    id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    sender_name: String,
    sender_role: String,
    body: String,
    is_task: bool,
    thread_followed: bool,
    delivery_state: String,
    stream_key: String,
    task_number: Option<i64>,
    task_status: Option<String>,
    attachments: Vec<MessageAttachment>,
    artifacts: Vec<Artifact>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct MessageAttachment {
    id: Uuid,
    message_id: Uuid,
    original_name: String,
    mime_type: String,
    size_bytes: i64,
    storage_path: String,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct Artifact {
    id: Uuid,
    message_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    creator_agent_id: Option<Uuid>,
    creator_agent_handle: Option<String>,
    kind: String,
    title: String,
    summary: String,
    content: String,
    metadata: Value,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct AttachmentUpload {
    original_name: String,
    mime_type: String,
    bytes: Vec<u8>,
}

#[derive(Debug, Serialize)]
struct Task {
    id: Uuid,
    number: i64,
    message_id: Uuid,
    channel_id: Uuid,
    title: String,
    status: String,
    channel_name: String,
    assignee_id: Option<Uuid>,
    assignee_name: Option<String>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct Reminder {
    id: Uuid,
    channel_id: Option<Uuid>,
    channel_name: Option<String>,
    creator_agent_id: Option<Uuid>,
    creator_agent_handle: Option<String>,
    thread_root_id: Option<Uuid>,
    message_id: Option<Uuid>,
    title: String,
    note: String,
    status: String,
    recurrence: String,
    due_at: DateTime<Utc>,
    fired_at: Option<DateTime<Utc>>,
    completed_at: Option<DateTime<Utc>>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct AgentSchedule {
    id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
    channel_id: Uuid,
    channel_name: String,
    channel_kind: String,
    thread_root_id: Option<Uuid>,
    title: String,
    prompt: String,
    cadence: String,
    status: String,
    next_run_at: DateTime<Utc>,
    last_run_at: Option<DateTime<Utc>>,
    last_work_item_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct AgentRun {
    id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
    work_item_id: Option<Uuid>,
    command: String,
    working_directory: String,
    status: String,
    pid: Option<i32>,
    exit_code: Option<i32>,
    log: String,
    input_tokens: i64,
    output_tokens: i64,
    cost_micros: i64,
    started_at: DateTime<Utc>,
    stopped_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct AgentRunPatch {
    id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
    work_item_id: Option<Uuid>,
    command: String,
    working_directory: String,
    status: String,
    pid: Option<i32>,
    exit_code: Option<i32>,
    input_tokens: i64,
    output_tokens: i64,
    cost_micros: i64,
    started_at: DateTime<Utc>,
    stopped_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct AgentActivity {
    id: Uuid,
    agent_id: Option<Uuid>,
    agent_handle: String,
    run_id: Option<Uuid>,
    kind: String,
    phase: String,
    status: String,
    title: String,
    summary: String,
    detail: String,
    metadata: Value,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct AgentWorkItem {
    id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
    channel_id: Option<Uuid>,
    channel_name: Option<String>,
    thread_root_id: Option<Uuid>,
    source_message_id: Option<Uuid>,
    task_id: Option<Uuid>,
    task_number: Option<i64>,
    source_kind: String,
    title: String,
    context: String,
    status: String,
    run_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct AgentWorkItemPatch {
    id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
    channel_id: Option<Uuid>,
    channel_name: Option<String>,
    thread_root_id: Option<Uuid>,
    source_message_id: Option<Uuid>,
    task_id: Option<Uuid>,
    task_number: Option<i64>,
    source_kind: String,
    title: String,
    status: String,
    run_id: Option<Uuid>,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
    completed_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct SupervisorStatus {
    pid: Option<i32>,
    status: String,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Serialize)]
struct LaunchAgentStatus {
    label: String,
    plist_path: String,
    installed: bool,
    loaded: bool,
}

#[derive(Debug)]
struct SupervisorCommand {
    id: Uuid,
    command_type: String,
    agent_id: Option<Uuid>,
    run_id: Option<Uuid>,
    work_item_id: Option<Uuid>,
}

#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AgentEvent {
    Message {
        channel: Option<String>,
        channel_id: Option<Uuid>,
        thread_root_id: Option<Uuid>,
        body: String,
        as_task: Option<bool>,
    },
    Activity {
        kind: Option<String>,
        title: String,
        detail: Option<String>,
    },
    TaskCreate {
        channel: Option<String>,
        channel_id: Option<Uuid>,
        title: String,
        body: Option<String>,
        thread_body: Option<String>,
        assign_self: Option<bool>,
        status: Option<String>,
    },
    TaskStatus {
        task_number: i64,
        status: String,
    },
    TaskClaim {
        task_number: i64,
        assignee_handle: Option<String>,
    },
    Silent {
        reason: Option<String>,
    },
    ReminderCreate {
        channel: Option<String>,
        channel_id: Option<Uuid>,
        thread_root_id: Option<Uuid>,
        message_id: Option<Uuid>,
        title: String,
        note: Option<String>,
        #[serde(alias = "when", alias = "dueAt", default)]
        due_at: Option<String>,
        #[serde(alias = "cadence", default)]
        recurrence: Option<String>,
    },
    ReminderCancel {
        reminder_id: Uuid,
    },
    Usage {
        #[serde(default)]
        input_tokens: Option<i64>,
        #[serde(default)]
        output_tokens: Option<i64>,
        #[serde(default)]
        total_tokens: Option<i64>,
        #[serde(default)]
        cost_micros: Option<i64>,
        #[serde(default)]
        cost_usd: Option<f64>,
    },
    MemoryAppend {
        body: String,
    },
    MemoryCompact {
        body: String,
    },
    ChannelCreate {
        name: String,
        description: Option<String>,
        agent_handles: Option<Vec<String>>,
    },
    ChannelInvite {
        channel: Option<String>,
        channel_id: Option<Uuid>,
        agent_handles: Vec<String>,
    },
    ProfileUpdate {
        display_name: Option<String>,
        role: Option<String>,
        avatar: Option<String>,
        description: Option<String>,
    },
    ArtifactCreate {
        channel: Option<String>,
        channel_id: Option<Uuid>,
        thread_root_id: Option<Uuid>,
        kind: String,
        title: String,
        summary: Option<String>,
        content: String,
        metadata: Option<Value>,
    },
}

#[derive(Debug, Serialize)]
struct Bootstrap {
    db_url: String,
    channels: Vec<Channel>,
    channel_members: Vec<ChannelMember>,
    agents: Vec<Agent>,
    messages: Vec<Message>,
    artifacts: Vec<Artifact>,
    tasks: Vec<Task>,
    reminders: Vec<Reminder>,
    agent_schedules: Vec<AgentSchedule>,
    agent_runs: Vec<AgentRun>,
    agent_work_items: Vec<AgentWorkItem>,
    agent_activities: Vec<AgentActivity>,
    supervisor: SupervisorStatus,
    launch_agent: LaunchAgentStatus,
}

type CommandResult<T> = Result<T, String>;

fn db_url() -> String {
    env::var("LOCAL_SLOCK_DATABASE_URL")
        .or_else(|_| env::var("DATABASE_URL"))
        .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned())
}

fn attachment_root_dir() -> CommandResult<PathBuf> {
    if let Ok(path) = env::var("LOCAL_SLOCK_ATTACHMENT_DIR") {
        return Ok(PathBuf::from(path));
    }
    let home = env::var("HOME").map_err(|_| "HOME is not set".to_owned())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("Application Support")
        .join("LocalSlock")
        .join("attachments"))
}

fn attachment_extension(original_name: &str) -> String {
    let path = PathBuf::from(original_name);
    let Some(extension) = path.extension().and_then(|value| value.to_str()) else {
        return String::new();
    };
    let sanitized: String = extension
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .take(12)
        .collect();
    if sanitized.is_empty() {
        String::new()
    } else {
        format!(".{}", sanitized.to_ascii_lowercase())
    }
}

fn write_attachment_file(
    message_id: Uuid,
    attachment_id: Uuid,
    original_name: &str,
    bytes: &[u8],
) -> CommandResult<String> {
    let root = attachment_root_dir()?;
    let message_dir = root.join(message_id.to_string());
    fs::create_dir_all(&message_dir).map_err(to_string)?;
    let path = message_dir.join(format!(
        "{}{}",
        attachment_id,
        attachment_extension(original_name)
    ));
    fs::write(&path, bytes).map_err(to_string)?;
    Ok(path.to_string_lossy().to_string())
}

fn format_attachment_size(size_bytes: i64) -> String {
    if size_bytes >= 1_000_000 {
        format!("{:.1}MB", size_bytes as f64 / 1_000_000.0)
    } else if size_bytes >= 1_000 {
        format!("{:.1}KB", size_bytes as f64 / 1_000.0)
    } else {
        format!("{size_bytes}B")
    }
}

async fn load_message_attachment_lines(
    pool: &PgPool,
    message_ids: &[Uuid],
) -> CommandResult<HashMap<Uuid, Vec<String>>> {
    if message_ids.is_empty() {
        return Ok(HashMap::new());
    }
    let rows = sqlx::query(
        r#"
        select id, message_id, original_name, mime_type, size_bytes, storage_path
        from message_attachments
        where message_id = any($1)
        order by created_at asc
        "#,
    )
    .bind(message_ids)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    let mut attachments_by_message: HashMap<Uuid, Vec<String>> = HashMap::new();
    for row in rows {
        let id: Uuid = row.get("id");
        let message_id: Uuid = row.get("message_id");
        let original_name: String = row.get("original_name");
        let mime_type: String = row.get("mime_type");
        let size_bytes: i64 = row.get("size_bytes");
        let storage_path: String = row.get("storage_path");
        attachments_by_message
            .entry(message_id)
            .or_default()
            .push(format!(
                "- attachment_id={} name=\"{}\" mime={} size={} local_path=\"{}\"",
                id,
                original_name.replace('"', "\\\""),
                mime_type,
                format_attachment_size(size_bytes),
                storage_path.replace('"', "\\\"")
            ));
    }
    Ok(attachments_by_message)
}

fn attachment_summary_sql() -> &'static str {
    r#"
    coalesce((
        select string_agg(
            'attachment_id=' || ma.id::text ||
            ' name=' || quote_literal(ma.original_name) ||
            ' mime=' || ma.mime_type ||
            ' size=' || ma.size_bytes::text ||
            ' local_path=' || quote_literal(ma.storage_path),
            E'\n'
            order by ma.created_at asc
        )
        from message_attachments ma
        where ma.message_id = m.id
    ), '') as attachment_summary
    "#
}

async fn notify_postgres(pool: &PgPool, channel: &str, payload: &str) -> CommandResult<()> {
    sqlx::query("select pg_notify($1, $2)")
        .bind(channel)
        .bind(payload)
        .execute(pool)
        .await
        .map_err(to_string)?;

    Ok(())
}

async fn notify_ui_refresh(pool: &PgPool, reason: &str) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "refresh", "reason": reason }).to_string(),
    )
    .await
}

async fn notify_ui_message_upsert(
    pool: &PgPool,
    message: &Message,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "message_upsert", "reason": reason, "message": message }).to_string(),
    )
    .await
}

async fn notify_ui_message_delta(
    pool: &PgPool,
    message_id: Uuid,
    append: &str,
    delivery_state: &str,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({
            "type": "message_delta",
            "reason": reason,
            "message_id": message_id,
            "append": append,
            "delivery_state": delivery_state
        })
        .to_string(),
    )
    .await
}

async fn notify_ui_message_delete(
    pool: &PgPool,
    message_id: Uuid,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "message_delete", "reason": reason, "message_id": message_id })
            .to_string(),
    )
    .await
}

async fn notify_ui_activity_upsert(
    pool: &PgPool,
    activity: &AgentActivity,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "activity_upsert", "reason": reason, "activity": activity }).to_string(),
    )
    .await
}

async fn notify_ui_agent_run_upsert(
    pool: &PgPool,
    run: &AgentRunPatch,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "agent_run_upsert", "reason": reason, "run": run }).to_string(),
    )
    .await
}

async fn notify_ui_work_item_upsert(
    pool: &PgPool,
    work_item: &AgentWorkItemPatch,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "work_item_upsert", "reason": reason, "work_item": work_item })
            .to_string(),
    )
    .await
}

async fn notify_ui_artifact_upsert(
    pool: &PgPool,
    artifact: &Artifact,
    reason: &str,
) -> CommandResult<()> {
    notify_postgres(
        pool,
        UI_REFRESH_CHANNEL,
        &json!({ "type": "artifact_upsert", "reason": reason, "artifact": artifact }).to_string(),
    )
    .await
}

async fn notify_ui_agent_run_changed(pool: &PgPool, run_id: Uuid, reason: &str) {
    if let Ok(run) = load_agent_run_patch(pool, run_id).await {
        let _ = notify_ui_agent_run_upsert(pool, &run, reason).await;
    } else {
        let _ = notify_ui_refresh(pool, reason).await;
    }
}

async fn notify_ui_work_item_changed(pool: &PgPool, work_item_id: Uuid, reason: &str) {
    if let Ok(work_item) = load_agent_work_item_patch(pool, work_item_id).await {
        let _ = notify_ui_work_item_upsert(pool, &work_item, reason).await;
        let _ = maybe_insert_work_item_system_message(pool, &work_item, reason).await;
    } else {
        let _ = notify_ui_refresh(pool, reason).await;
    }
}

fn status_label(status: &str) -> String {
    status.replace('_', " ")
}

async fn insert_system_message(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    body: impl AsRef<str>,
) -> CommandResult<Uuid> {
    let body = body.as_ref().trim();
    if body.is_empty() {
        return Err("system message body is empty".to_owned());
    }
    let message_id: Uuid = sqlx::query_scalar(
        r#"
        insert into messages (channel_id, thread_root_id, sender_name, sender_role, body, is_task)
        values ($1, $2, 'LocalSlock', 'system', $3, false)
        returning id
        "#,
    )
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(body)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    if let Ok(message) = load_message(pool, message_id).await {
        let _ = notify_ui_message_upsert(pool, &message, "system_message").await;
    } else {
        let _ = notify_ui_refresh(pool, "system_message").await;
    }
    Ok(message_id)
}

async fn maybe_insert_work_item_system_message(
    pool: &PgPool,
    work_item: &AgentWorkItemPatch,
    reason: &str,
) -> CommandResult<()> {
    // Conversation-triggered agent turns are attention events, not timeline-level tasks.
    // Keep normal lifecycle messages for explicit task-backed work only; still surface
    // exceptional failures/cancellations for conversational turns.
    if work_item.task_number.is_none()
        && !matches!(
            reason,
            "work_item_failed" | "work_item_cancelled" | "work_item_finished"
        )
    {
        return Ok(());
    }
    let Some(channel_id) = work_item.channel_id else {
        return Ok(());
    };
    let thread_root_id = work_item.thread_root_id.or(work_item.source_message_id);
    let object_label = work_item
        .task_number
        .map(|number| format!("task run for task #{number}"))
        .unwrap_or_else(|| "agent request".to_owned());
    let title = work_item.title.trim();
    let title_suffix = if title.is_empty() {
        String::new()
    } else {
        format!(": {title}")
    };
    let body = match reason {
        "work_item_created" | "work_item_queued" => {
            format!(
                "@{} queued {}{}",
                work_item.agent_handle, object_label, title_suffix
            )
        }
        "work_item_running" => {
            format!(
                "@{} started {}{}",
                work_item.agent_handle, object_label, title_suffix
            )
        }
        "work_item_cancelling" => {
            format!(
                "@{} is stopping {}{}",
                work_item.agent_handle, object_label, title_suffix
            )
        }
        "work_item_cancelled" => {
            format!(
                "@{} cancelled {}{}",
                work_item.agent_handle, object_label, title_suffix
            )
        }
        "work_item_failed" => {
            format!(
                "@{} failed {}{}",
                work_item.agent_handle, object_label, title_suffix
            )
        }
        "work_item_finished" => match work_item.status.as_str() {
            "done" => format!(
                "@{} completed {}{}",
                work_item.agent_handle, object_label, title_suffix
            ),
            "failed" => format!(
                "@{} failed {}{}",
                work_item.agent_handle, object_label, title_suffix
            ),
            "cancelled" => format!(
                "@{} cancelled {}{}",
                work_item.agent_handle, object_label, title_suffix
            ),
            "silent" => return Ok(()),
            _ => return Ok(()),
        },
        _ => return Ok(()),
    };
    insert_system_message(pool, channel_id, thread_root_id, body).await?;
    Ok(())
}

async fn notify_supervisor_wake(pool: &PgPool) -> CommandResult<()> {
    notify_postgres(pool, SUPERVISOR_WAKE_CHANNEL, "wake").await
}

fn spawn_reminder_worker(pool: PgPool) {
    tauri::async_runtime::spawn(async move {
        loop {
            if let Err(err) = process_due_reminders(&pool).await {
                eprintln!("LocalSlock reminder worker failed: {err}");
            }
            if let Err(err) = process_due_agent_schedules(&pool).await {
                eprintln!("LocalSlock schedule worker failed: {err}");
            }
            sleep(Duration::from_secs(15)).await;
        }
    });
}

async fn process_due_reminders(pool: &PgPool) -> CommandResult<()> {
    let rows = sqlx::query(
        r#"
        update reminders
        set status = case when recurrence = 'none' then 'fired' else 'scheduled' end,
            fired_at = now(),
            due_at = case
                when recurrence = 'daily' then now() + interval '1 day'
                when recurrence = 'weekly' then now() + interval '7 days'
                else due_at
            end,
            updated_at = now()
        where id in (
            select id
            from reminders
            where status = 'scheduled'
              and due_at <= now()
            order by due_at asc
            for update skip locked
            limit 12
        )
        returning id, channel_id, creator_agent_id, thread_root_id, title, note, recurrence, status, due_at
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    let fired_any = !rows.is_empty();
    for row in rows {
        let reminder_id: Uuid = row.get("id");
        let channel_id: Option<Uuid> = row.get("channel_id");
        let creator_agent_id: Option<Uuid> = row.get("creator_agent_id");
        let thread_root_id: Option<Uuid> = row.get("thread_root_id");
        let title: String = row.get("title");
        let note: String = row.get("note");
        let recurrence: String = row.get("recurrence");
        let status: String = row.get("status");
        let next_due_at: DateTime<Utc> = row.get("due_at");
        insert_reminder_event(
            pool,
            reminder_id,
            "fired",
            if recurrence == "none" {
                String::new()
            } else {
                format!("next_due_at={}", next_due_at.to_rfc3339())
            },
        )
        .await?;

        if let Some(channel_id) = channel_id {
            let mut body = format!("Reminder: {title}");
            if !note.trim().is_empty() {
                body.push_str(&format!("\n{}", note.trim()));
            }
            if recurrence != "none" && status == "scheduled" {
                body.push_str(&format!("\nNext reminder: {}", next_due_at.to_rfc3339()));
            }
            if let Ok(message_id) =
                insert_system_message(pool, channel_id, thread_root_id, body).await
            {
                if let Some(agent_id) = creator_agent_id {
                    let _ = dispatch_due_reminder_to_agent(
                        pool,
                        reminder_id,
                        agent_id,
                        channel_id,
                        thread_root_id,
                        message_id,
                        &title,
                        &note,
                    )
                    .await;
                }
            }
        }
    }
    if fired_any {
        let _ = notify_ui_refresh(pool, "reminder_due").await;
    }
    Ok(())
}

async fn dispatch_due_reminder_to_agent(
    pool: &PgPool,
    reminder_id: Uuid,
    agent_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    source_message_id: Uuid,
    title: &str,
    note: &str,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        insert into channel_members (channel_id, agent_id)
        values ($1, $2)
        on conflict (channel_id, agent_id) do nothing
        "#,
    )
    .bind(channel_id)
    .bind(agent_id)
    .execute(pool)
    .await
    .map_err(to_string)?;

    let context = [
        format!("Reminder fired: {reminder_id}"),
        format!("Source message id: {source_message_id}"),
        format!("Reminder title: {}", title.trim()),
        if note.trim().is_empty() {
            "Reminder note:".to_owned()
        } else {
            format!("Reminder note:\n{}", note.trim())
        },
        "This reminder was created by you earlier. Check whether follow-up work is now needed. If not, use LOCAL_SLOCK_SILENT_REPLY.".to_owned(),
    ]
    .join("\n");
    let work_item_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_work_items (
            agent_id, channel_id, thread_root_id, source_message_id, source_kind, title, context, status
        )
        values ($1, $2, $3, $4, 'reminder', $5, $6, 'queued')
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(channel_id)
    .bind(thread_root_id.or(Some(source_message_id)))
    .bind(source_message_id)
    .bind(format!("Reminder due: {}", title.trim()))
    .bind(context)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    notify_ui_work_item_changed(pool, work_item_id, "work_item_created").await;
    let scheduled = enqueue_agent_work_if_available(pool, agent_id, work_item_id).await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        None,
        "reminder",
        if scheduled {
            "Reminder follow-up dispatched"
        } else {
            "Reminder follow-up queued"
        },
        json!({
            "reminder_id": reminder_id,
            "work_item_id": work_item_id,
            "source_message_id": source_message_id
        })
        .to_string(),
    )
    .await?;
    Ok(())
}

async fn process_due_agent_schedules(pool: &PgPool) -> CommandResult<()> {
    let rows = sqlx::query(
        r#"
        update agent_schedules s
        set last_run_at = now(),
            next_run_at = case
                when s.cadence = 'hourly' then now() + interval '1 hour'
                when s.cadence = 'daily' then now() + interval '1 day'
                when s.cadence = 'weekly' then now() + interval '7 days'
                else now() + interval '1 day'
            end,
            updated_at = now()
        where s.id in (
            select id
            from agent_schedules
            where status = 'active'
              and next_run_at <= now()
            order by next_run_at asc
            for update skip locked
            limit 8
        )
        returning
            s.id,
            s.agent_id,
            (select handle from agents where id = s.agent_id) as agent_handle,
            s.channel_id,
            (select name from channels where id = s.channel_id) as channel_name,
            s.thread_root_id,
            s.title,
            s.prompt,
            s.cadence,
            s.next_run_at
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    let fired_any = !rows.is_empty();
    for row in rows {
        let schedule_id: Uuid = row.get("id");
        let agent_id: Uuid = row.get("agent_id");
        let agent_handle: String = row.get("agent_handle");
        let channel_id: Uuid = row.get("channel_id");
        let channel_name: String = row.get("channel_name");
        let thread_root_id: Option<Uuid> = row.get("thread_root_id");
        let title: String = row.get("title");
        let prompt: String = row.get("prompt");
        let cadence: String = row.get("cadence");
        let next_run_at: DateTime<Utc> = row.get("next_run_at");

        let system_body = format!(
            "Scheduled routine for @{agent_handle}: {title}\nNext run: {}",
            next_run_at.to_rfc3339()
        );
        let source_message_id =
            insert_system_message(pool, channel_id, thread_root_id, system_body).await?;
        let work_thread_root_id = thread_root_id.or(Some(source_message_id));
        let context = build_schedule_work_context(
            schedule_id,
            &agent_handle,
            &channel_name,
            work_thread_root_id,
            source_message_id,
            &title,
            &prompt,
            &cadence,
        );
        let work_item_id: Uuid = sqlx::query_scalar(
            r#"
            insert into agent_work_items (
                agent_id, channel_id, thread_root_id, source_message_id, source_kind, title, context, status
            )
            values ($1, $2, $3, $4, 'schedule', $5, $6, 'queued')
            returning id
            "#,
        )
        .bind(agent_id)
        .bind(channel_id)
        .bind(work_thread_root_id)
        .bind(source_message_id)
        .bind(&title)
        .bind(&context)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;

        sqlx::query(
            "update agent_schedules set last_work_item_id = $2, updated_at = now() where id = $1",
        )
        .bind(schedule_id)
        .bind(work_item_id)
        .execute(pool)
        .await
        .map_err(to_string)?;

        notify_ui_work_item_changed(pool, work_item_id, "work_item_created").await;
        let scheduled = enqueue_agent_work_if_available(pool, agent_id, work_item_id).await?;
        record_agent_activity(
            pool,
            Some(agent_id),
            None,
            "schedule",
            if scheduled {
                "Scheduled routine dispatched"
            } else {
                "Scheduled routine queued"
            },
            json!({
                "schedule_id": schedule_id,
                "work_item_id": work_item_id,
                "channel": format!("#{channel_name}"),
                "cadence": cadence,
                "next_run_at": next_run_at.to_rfc3339()
            })
            .to_string(),
        )
        .await?;
    }
    if fired_any {
        let _ = notify_ui_refresh(pool, "agent_schedule_due").await;
    }
    Ok(())
}

fn spawn_ui_refresh_listener(app: tauri::AppHandle, database_url: String) {
    tauri::async_runtime::spawn(async move {
        loop {
            match PgListener::connect(&database_url).await {
                Ok(mut listener) => {
                    if let Err(err) = listener.listen(UI_REFRESH_CHANNEL).await {
                        eprintln!("LocalSlock UI refresh listener failed to listen: {err}");
                    } else {
                        loop {
                            match listener.recv().await {
                                Ok(notification) => {
                                    let _ = app.emit(UI_REFRESH_EVENT, notification.payload());
                                }
                                Err(err) => {
                                    eprintln!("LocalSlock UI refresh listener disconnected: {err}");
                                    break;
                                }
                            }
                        }
                    }
                }
                Err(err) => {
                    eprintln!("LocalSlock UI refresh listener failed to connect: {err}");
                }
            }
            sleep(Duration::from_secs(2)).await;
        }
    });
}

async fn migrate(pool: &PgPool) -> Result<(), sqlx::Error> {
    sqlx::query("create extension if not exists pgcrypto")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        create table if not exists agents (
            id uuid primary key default gen_random_uuid(),
            handle text not null unique,
            display_name text not null,
            role text not null default 'agent',
            status text not null default 'idle',
            runtime text not null default 'codex',
            model text not null default 'gpt-5.5',
            avatar text not null default '',
            description text not null default '',
            created_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "alter table agents add column if not exists launch_command text not null default ''",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table agents add column if not exists working_directory text not null default ''",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table agents add column if not exists daily_budget_micros bigint not null default 0",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists channels (
            id uuid primary key default gen_random_uuid(),
            name text not null unique,
            description text not null default '',
            unread_count integer not null default 0,
            created_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table channels add column if not exists kind text not null default 'channel'",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table channels add column if not exists dm_agent_id uuid references agents(id) on delete cascade",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create unique index if not exists channels_dm_unique
        on channels(dm_agent_id)
        where kind = 'dm' and dm_agent_id is not null
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists messages (
            id uuid primary key default gen_random_uuid(),
            channel_id uuid not null references channels(id) on delete cascade,
            thread_root_id uuid references messages(id) on delete cascade,
            sender_agent_id uuid references agents(id) on delete set null,
            sender_name text not null,
            sender_role text not null default 'human',
            body text not null,
            is_task boolean not null default false,
            thread_followed boolean not null default true,
            created_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table messages add column if not exists thread_followed boolean not null default true",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table messages add column if not exists delivery_state text not null default 'complete'",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table messages add column if not exists stream_key text not null default ''",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table messages add column if not exists updated_at timestamptz not null default now()",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        r#"
        create unique index if not exists messages_stream_key_unique
        on messages(stream_key)
        where stream_key <> ''
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists message_attachments (
            id uuid primary key default gen_random_uuid(),
            message_id uuid not null references messages(id) on delete cascade,
            original_name text not null,
            mime_type text not null,
            size_bytes bigint not null,
            storage_path text not null,
            created_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "create index if not exists message_attachments_message_id_idx on message_attachments(message_id)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists artifacts (
            id uuid primary key default gen_random_uuid(),
            message_id uuid not null references messages(id) on delete cascade,
            channel_id uuid not null references channels(id) on delete cascade,
            thread_root_id uuid references messages(id) on delete set null,
            creator_agent_id uuid references agents(id) on delete set null,
            kind text not null,
            title text not null,
            summary text not null default '',
            content text not null,
            metadata jsonb not null default '{}'::jsonb,
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query("create index if not exists artifacts_message_id_idx on artifacts(message_id)")
        .execute(pool)
        .await?;
    sqlx::query("create index if not exists artifacts_channel_id_idx on artifacts(channel_id)")
        .execute(pool)
        .await?;

    sqlx::query(
        r#"
        create table if not exists runtime_sessions (
            id uuid primary key default gen_random_uuid(),
            agent_id uuid not null references agents(id) on delete cascade,
            runtime text not null,
            provider_thread_id text not null,
            status text not null default 'idle',
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now(),
            unique(agent_id, runtime)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists tasks (
            id uuid primary key default gen_random_uuid(),
            number bigserial not null unique,
            message_id uuid not null unique references messages(id) on delete cascade,
            channel_id uuid not null references channels(id) on delete cascade,
            title text not null,
            status text not null default 'todo',
            assignee_agent_id uuid references agents(id) on delete set null,
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists reminders (
            id uuid primary key default gen_random_uuid(),
            channel_id uuid references channels(id) on delete set null,
            creator_agent_id uuid references agents(id) on delete set null,
            thread_root_id uuid references messages(id) on delete set null,
            message_id uuid references messages(id) on delete set null,
            title text not null,
            note text not null default '',
            status text not null default 'scheduled',
            recurrence text not null default 'none',
            due_at timestamptz not null,
            fired_at timestamptz,
            completed_at timestamptz,
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table reminders add column if not exists creator_agent_id uuid references agents(id) on delete set null",
    )
    .execute(pool)
    .await?;
    sqlx::query("create index if not exists reminders_due_idx on reminders(status, due_at)")
        .execute(pool)
        .await?;
    sqlx::query(
        r#"
        create table if not exists reminder_events (
            id uuid primary key default gen_random_uuid(),
            reminder_id uuid not null references reminders(id) on delete cascade,
            event_type text not null,
            detail text not null default '',
            created_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists agent_schedules (
            id uuid primary key default gen_random_uuid(),
            agent_id uuid not null references agents(id) on delete cascade,
            channel_id uuid not null references channels(id) on delete cascade,
            thread_root_id uuid references messages(id) on delete set null,
            title text not null,
            prompt text not null default '',
            cadence text not null default 'daily',
            status text not null default 'active',
            next_run_at timestamptz not null,
            last_run_at timestamptz,
            last_work_item_id uuid,
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "create index if not exists agent_schedules_due_idx on agent_schedules(status, next_run_at)",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists agent_runs (
            id uuid primary key default gen_random_uuid(),
            agent_id uuid not null references agents(id) on delete cascade,
            command text not null,
            working_directory text not null default '',
            status text not null default 'starting',
            pid integer,
            exit_code integer,
            log text not null default '',
            started_at timestamptz not null default now(),
            stopped_at timestamptz
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists agent_activities (
            id uuid primary key default gen_random_uuid(),
            agent_id uuid references agents(id) on delete set null,
            agent_handle text not null default '',
            run_id uuid references agent_runs(id) on delete set null,
            kind text not null,
            phase text not null default 'event',
            status text not null default 'info',
            title text not null,
            summary text not null default '',
            detail text not null default '',
            metadata jsonb not null default '{}'::jsonb,
            created_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;

    for statement in [
        "alter table agent_activities add column if not exists phase text not null default 'event'",
        "alter table agent_activities add column if not exists status text not null default 'info'",
        "alter table agent_activities add column if not exists summary text not null default ''",
        "alter table agent_activities add column if not exists metadata jsonb not null default '{}'::jsonb",
    ] {
        sqlx::query(statement).execute(pool).await?;
    }
    sqlx::query(
        r#"
        update agent_activities
        set phase = case
                when kind = 'thinking' then 'thinking'
                when kind = 'command' then 'command'
                when kind = 'file_edit' then 'file_edit'
                when kind = 'tools' then 'tools'
                when kind in ('error', 'event_error', 'run_error') then 'error'
                when kind = 'run' then 'runtime'
                when kind in ('dispatch', 'mention', 'dm', 'task') then 'work'
                when kind = 'profile' then 'profile'
                else 'acting'
            end,
            status = case
                when kind in ('error', 'event_error', 'run_error')
                    or lower(title) like '%failed%'
                    or lower(title) like '%error%'
                    or lower(title) like '%rejected%' then 'error'
                when lower(title) like '%cancel%' or lower(title) like '%stop%' then 'warning'
                when lower(title) like '%completed%'
                    or lower(title) like '%complete%'
                    or lower(title) like '%done%'
                    or lower(title) like '%exited%'
                    or lower(title) like '%finished%'
                    or lower(title) like '%ready%'
                    or lower(title) like '%accepted%' then 'success'
                when lower(title) like '%running%'
                    or lower(title) like '%editing%'
                    or lower(title) like '%started%'
                    or lower(title) like '%queued%'
                    or lower(title) like '%dispatched%' then 'active'
                else 'info'
            end,
            summary = title,
            metadata = case
                when detail <> '' and metadata = '{}'::jsonb then jsonb_build_object('detail', detail)
                else metadata
            end
        where summary = '' or metadata = '{}'::jsonb
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists agent_work_items (
            id uuid primary key default gen_random_uuid(),
            agent_id uuid not null references agents(id) on delete cascade,
            channel_id uuid references channels(id) on delete set null,
            thread_root_id uuid references messages(id) on delete set null,
            source_message_id uuid references messages(id) on delete set null,
            task_id uuid references tasks(id) on delete set null,
            source_kind text not null default 'manual',
            title text not null,
            context text not null default '',
            status text not null default 'queued',
            run_id uuid references agent_runs(id) on delete set null,
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now(),
            completed_at timestamptz
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "alter table agent_work_items add column if not exists source_message_id uuid references messages(id) on delete set null",
    )
    .execute(pool)
    .await?;
    sqlx::query(
        "alter table agent_work_items add column if not exists source_kind text not null default 'manual'",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists agent_thread_subscriptions (
            agent_id uuid not null references agents(id) on delete cascade,
            channel_id uuid not null references channels(id) on delete cascade,
            thread_root_id uuid not null references messages(id) on delete cascade,
            source_kind text not null default 'manual',
            last_source_message_id uuid references messages(id) on delete set null,
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now(),
            primary key (agent_id, thread_root_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "alter table agent_runs add column if not exists work_item_id uuid references agent_work_items(id) on delete set null",
    )
    .execute(pool)
    .await?;
    for statement in [
        "alter table agent_runs add column if not exists input_tokens bigint not null default 0",
        "alter table agent_runs add column if not exists output_tokens bigint not null default 0",
        "alter table agent_runs add column if not exists cost_micros bigint not null default 0",
    ] {
        sqlx::query(statement).execute(pool).await?;
    }
    backfill_agent_run_usage_from_logs(pool).await?;

    sqlx::query(
        r#"
        create table if not exists agent_event_receipts (
            run_id uuid not null references agent_runs(id) on delete cascade,
            event_json text not null,
            created_at timestamptz not null default now(),
            primary key (run_id, event_json)
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists supervisor_commands (
            id uuid primary key default gen_random_uuid(),
            command_type text not null,
            agent_id uuid references agents(id) on delete cascade,
            run_id uuid references agent_runs(id) on delete cascade,
            work_item_id uuid references agent_work_items(id) on delete set null,
            status text not null default 'pending',
            error text not null default '',
            created_at timestamptz not null default now(),
            updated_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        "alter table supervisor_commands add column if not exists work_item_id uuid references agent_work_items(id) on delete set null",
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists supervisor_state (
            id integer primary key default 1 check (id = 1),
            pid integer,
            status text not null default 'offline',
            updated_at timestamptz not null default now()
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists channel_read_state (
            channel_id uuid primary key references channels(id) on delete cascade,
            last_read_at timestamptz not null default '-infinity'::timestamptz
        )
        "#,
    )
    .execute(pool)
    .await?;

    sqlx::query(
        r#"
        create table if not exists channel_members (
            channel_id uuid not null references channels(id) on delete cascade,
            agent_id uuid not null references agents(id) on delete cascade,
            created_at timestamptz not null default now(),
            primary key (channel_id, agent_id)
        )
        "#,
    )
    .execute(pool)
    .await?;

    Ok(())
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppState>) -> CommandResult<Bootstrap> {
    let channels = load_channels(&state.pool).await?;
    let channel_members = load_channel_members(&state.pool).await?;
    let agents = load_agents(&state.pool).await?;
    let messages = load_messages(&state.pool).await?;
    let artifacts = load_artifacts(&state.pool).await?;
    let tasks = load_tasks(&state.pool).await?;
    let reminders = load_reminders(&state.pool).await?;
    let agent_schedules = load_agent_schedules(&state.pool).await?;
    let agent_runs = load_agent_runs(&state.pool).await?;
    let agent_work_items = load_agent_work_items(&state.pool).await?;
    let agent_activities = load_agent_activities(&state.pool).await?;
    let supervisor = load_supervisor_status(&state.pool).await?;
    let launch_agent = load_launch_agent_status()?;

    Ok(Bootstrap {
        db_url: state.db_url.clone(),
        channels,
        channel_members,
        agents,
        messages,
        artifacts,
        tasks,
        reminders,
        agent_schedules,
        agent_runs,
        agent_work_items,
        agent_activities,
        supervisor,
        launch_agent,
    })
}

#[tauri::command]
async fn check_runtime(runtime: String) -> CommandResult<RuntimeCheck> {
    let runtime = runtime.trim().to_owned();
    let command = match runtime.as_str() {
        "codex" => "codex",
        "claude" => "claude",
        "kimi" => "kimi",
        _ => {
            return Ok(RuntimeCheck {
                runtime,
                command: String::new(),
                available: false,
                detail: "Unknown runtime".to_owned(),
            });
        }
    };

    let script = format!(
        "if command -v {command} >/dev/null 2>&1; then {command} --version 2>&1 | head -n 1; else echo '{command} not found in PATH' >&2; exit 127; fi"
    );
    let output = StdCommand::new("/bin/zsh")
        .arg("-lc")
        .arg(script)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(to_string)?;

    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let detail = if !stdout.is_empty() {
        stdout
    } else if !stderr.is_empty() {
        stderr
    } else if output.status.success() {
        format!("{command} found")
    } else {
        format!("{command} unavailable")
    };

    Ok(RuntimeCheck {
        runtime,
        command: command.to_owned(),
        available: output.status.success(),
        detail,
    })
}

#[tauri::command]
async fn create_channel(name: String, state: State<'_, AppState>) -> CommandResult<()> {
    let normalized = normalize_channel_name(&name);
    if normalized.is_empty() {
        return Err("channel name is empty".to_owned());
    }

    sqlx::query(
        r#"
        insert into channels (name, description, kind)
        values ($1, 'Local channel', 'channel')
        on conflict (name) do nothing
        "#,
    )
    .bind(normalized)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

#[tauri::command]
async fn update_channel(
    channel_id: Uuid,
    name: String,
    description: String,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let normalized = normalize_channel_name(&name);
    if normalized.is_empty() {
        return Err("channel name is empty".to_owned());
    }

    let kind: Option<String> = sqlx::query_scalar("select kind from channels where id = $1")
        .bind(channel_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(to_string)?;
    match kind.as_deref() {
        Some("dm") => return Err("direct messages cannot be renamed".to_owned()),
        Some(_) => {}
        None => return Err("channel does not exist".to_owned()),
    }

    sqlx::query(
        r#"
        update channels
        set name = $2, description = $3
        where id = $1
        "#,
    )
    .bind(channel_id)
    .bind(normalized)
    .bind(description.trim())
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

#[tauri::command]
async fn set_channel_agent_membership(
    channel_id: Uuid,
    agent_id: Uuid,
    member: bool,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let channel_row = sqlx::query("select name, kind from channels where id = $1")
        .bind(channel_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(to_string)?;
    let Some(channel_row) = channel_row else {
        return Err("channel does not exist".to_owned());
    };
    let channel_name: String = channel_row.get("name");
    let channel_kind: String = channel_row.get("kind");
    if channel_kind == "dm" {
        return Err("direct message membership is fixed".to_owned());
    }

    let agent_handle: Option<String> =
        sqlx::query_scalar("select handle from agents where id = $1")
            .bind(agent_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(to_string)?;
    if agent_handle.is_none() {
        return Err("agent does not exist".to_owned());
    }

    if member {
        sqlx::query(
            r#"
            insert into channel_members (channel_id, agent_id)
            values ($1, $2)
            on conflict (channel_id, agent_id) do nothing
            "#,
        )
        .bind(channel_id)
        .bind(agent_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    } else {
        sqlx::query("delete from channel_members where channel_id = $1 and agent_id = $2")
            .bind(channel_id)
            .bind(agent_id)
            .execute(&state.pool)
            .await
            .map_err(to_string)?;
    }

    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "membership",
        if member {
            "Agent joined channel"
        } else {
            "Agent left channel"
        },
        format!("#{channel_name}"),
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn delete_channel(channel_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    delete_channel_in_pool(&state.pool, channel_id).await
}

async fn delete_channel_in_pool(pool: &PgPool, channel_id: Uuid) -> CommandResult<()> {
    let result = sqlx::query("delete from channels where id = $1")
        .bind(channel_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    if result.rows_affected() == 0 {
        return Err("channel does not exist".to_owned());
    }

    let _ = notify_ui_refresh(pool, "channel_deleted").await;

    Ok(())
}

#[tauri::command]
async fn open_dm_with_agent(agent_id: Uuid, state: State<'_, AppState>) -> CommandResult<String> {
    open_dm_with_agent_in_pool(&state.pool, agent_id).await
}

#[tauri::command]
async fn artifact_read(artifact_id: Uuid, state: State<'_, AppState>) -> CommandResult<Artifact> {
    load_artifact(&state.pool, artifact_id).await
}

async fn open_dm_with_agent_in_pool(pool: &PgPool, agent_id: Uuid) -> CommandResult<String> {
    let mut tx = pool.begin().await.map_err(to_string)?;
    let agent_row = sqlx::query("select handle from agents where id = $1")
        .bind(agent_id)
        .fetch_optional(&mut *tx)
        .await
        .map_err(to_string)?;
    let Some(agent_row) = agent_row else {
        return Err("agent does not exist".to_owned());
    };
    let agent_handle: String = agent_row.get("handle");

    let existing: Option<Uuid> = sqlx::query_scalar(
        "select id from channels where kind = 'dm' and dm_agent_id = $1 limit 1",
    )
    .bind(agent_id)
    .fetch_optional(&mut *tx)
    .await
    .map_err(to_string)?;
    if let Some(channel_id) = existing {
        tx.commit().await.map_err(to_string)?;
        return Ok(channel_id.to_string());
    }

    let channel_id: Uuid = sqlx::query_scalar(
        r#"
        insert into channels (name, description, kind, dm_agent_id)
        values ($1, $2, 'dm', $3)
        returning id
        "#,
    )
    .bind(format!("dm:{agent_id}"))
    .bind(format!("Direct message with @{agent_handle}"))
    .bind(agent_id)
    .fetch_one(&mut *tx)
    .await
    .map_err(to_string)?;

    sqlx::query(
        r#"
        insert into channel_members (channel_id, agent_id)
        values ($1, $2)
        on conflict (channel_id, agent_id) do nothing
        "#,
    )
    .bind(channel_id)
    .bind(agent_id)
    .execute(&mut *tx)
    .await
    .map_err(to_string)?;

    tx.commit().await.map_err(to_string)?;
    record_agent_activity(
        pool,
        Some(agent_id),
        None,
        "dm",
        "Direct message opened",
        format!("@{agent_handle}"),
    )
    .await?;

    Ok(channel_id.to_string())
}

#[tauri::command]
async fn create_agent(
    handle: String,
    display_name: String,
    role: Option<String>,
    runtime: String,
    model: String,
    avatar: Option<String>,
    description: Option<String>,
    launch_command: String,
    working_directory: String,
    daily_budget_micros: Option<i64>,
    state: State<'_, AppState>,
) -> CommandResult<String> {
    let normalized_handle = handle.trim().trim_start_matches('@');
    if normalized_handle.is_empty() {
        return Err("agent handle is empty".to_owned());
    }
    let display_name = if display_name.trim().is_empty() {
        normalized_handle
    } else {
        display_name.trim()
    };
    let role = role
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("agent");
    let avatar = avatar
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            normalized_handle
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "A".to_owned())
        });
    let description = description
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("Local agent");
    let daily_budget_micros = daily_budget_micros.unwrap_or_default().max(0);
    let working_directory = working_directory.trim();
    ensure_agent_workspace(working_directory, normalized_handle)?;

    let agent_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agents (
            handle, display_name, role, status, runtime, model, avatar, description,
            launch_command, working_directory, daily_budget_micros
        )
        values ($1, $2, $3, 'idle', $4, $5, $6, $7, $8, $9, $10)
        on conflict (handle) do update set
            display_name = excluded.display_name,
            role = excluded.role,
            runtime = excluded.runtime,
            model = excluded.model,
            avatar = excluded.avatar,
            description = excluded.description,
            launch_command = excluded.launch_command,
            working_directory = excluded.working_directory,
            daily_budget_micros = excluded.daily_budget_micros
        returning id
        "#,
    )
    .bind(normalized_handle)
    .bind(display_name)
    .bind(role)
    .bind(runtime.trim())
    .bind(model.trim())
    .bind(avatar)
    .bind(description)
    .bind(launch_command.trim())
    .bind(working_directory)
    .bind(daily_budget_micros)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "profile",
        "Agent profile saved",
        format!("runtime={} model={}", runtime.trim(), model.trim()),
    )
    .await?;

    Ok(agent_id.to_string())
}

#[tauri::command]
async fn update_agent(
    agent_id: Uuid,
    handle: String,
    display_name: String,
    role: Option<String>,
    runtime: String,
    model: String,
    avatar: Option<String>,
    description: String,
    launch_command: String,
    working_directory: String,
    daily_budget_micros: Option<i64>,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let normalized_handle = handle.trim().trim_start_matches('@');
    if normalized_handle.is_empty() {
        return Err("agent handle is empty".to_owned());
    }
    let display_name = if display_name.trim().is_empty() {
        normalized_handle
    } else {
        display_name.trim()
    };
    let role = role
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("agent");
    let avatar = avatar
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| {
            normalized_handle
                .chars()
                .next()
                .map(|c| c.to_uppercase().to_string())
                .unwrap_or_else(|| "A".to_owned())
        });
    let daily_budget_micros = daily_budget_micros.unwrap_or_default().max(0);
    let working_directory = working_directory.trim();
    ensure_agent_workspace(working_directory, normalized_handle)?;

    sqlx::query(
        r#"
        update agents
        set handle = $2,
            display_name = $3,
            role = $4,
            runtime = $5,
            model = $6,
            avatar = $7,
            description = $8,
            launch_command = $9,
            working_directory = $10,
            daily_budget_micros = $11
        where id = $1
        "#,
    )
    .bind(agent_id)
    .bind(normalized_handle)
    .bind(display_name)
    .bind(role)
    .bind(runtime.trim())
    .bind(model.trim())
    .bind(avatar)
    .bind(description.trim())
    .bind(launch_command.trim())
    .bind(working_directory)
    .bind(daily_budget_micros)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "profile",
        "Agent profile updated",
        format!("runtime={} model={}", runtime.trim(), model.trim()),
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn delete_agent(agent_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    delete_agent_in_pool(&state.pool, agent_id).await
}

async fn delete_agent_in_pool(pool: &PgPool, agent_id: Uuid) -> CommandResult<()> {
    let active_run: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from agent_runs
        where agent_id = $1
          and stopped_at is null
          and status in ('starting', 'running', 'stopping')
        limit 1
        "#,
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;

    if active_run.is_some() {
        return Err("stop the agent before deleting it".to_owned());
    }

    let agent_handle: Option<String> =
        sqlx::query_scalar("select handle from agents where id = $1")
            .bind(agent_id)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?;
    let Some(agent_handle) = agent_handle else {
        return Err("agent does not exist".to_owned());
    };

    record_agent_activity(
        pool,
        Some(agent_id),
        None,
        "profile",
        "Agent profile deleted",
        format!("@{agent_handle}"),
    )
    .await?;

    let result = sqlx::query("delete from agents where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    if result.rows_affected() == 0 {
        return Err("agent does not exist".to_owned());
    }

    let _ = notify_ui_refresh(pool, "agent_deleted").await;

    Ok(())
}

#[tauri::command]
async fn start_agent(agent_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    let active_run: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from agent_runs
        where agent_id = $1
          and stopped_at is null
          and status in ('starting', 'running', 'stopping')
        limit 1
        "#,
    )
    .bind(agent_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(to_string)?;

    if active_run.is_some() {
        return Err("agent already has an active run".to_owned());
    }

    let pending_start: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from supervisor_commands
        where command_type = 'start_agent'
          and agent_id = $1
          and status in ('pending', 'running')
        limit 1
        "#,
    )
    .bind(agent_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(to_string)?;

    if pending_start.is_some() {
        return Err("agent already has a pending start command".to_owned());
    }

    sqlx::query(
        r#"
        insert into supervisor_commands (command_type, agent_id)
        values ('start_agent', $1)
        "#,
    )
    .bind(agent_id)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;
    let _ = notify_supervisor_wake(&state.pool).await;
    let _ = notify_ui_refresh(&state.pool, "supervisor_command").await;
    sqlx::query("update agents set status = 'queued' where id = $1")
        .bind(agent_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "run",
        "Start queued",
        "Waiting for supervisor to launch the agent",
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn dispatch_agent_work(
    agent_id: Uuid,
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
    task_id: Option<Uuid>,
    title: String,
    context: String,
    state: State<'_, AppState>,
) -> CommandResult<Uuid> {
    let agent_handle: Option<String> =
        sqlx::query_scalar("select handle from agents where id = $1")
            .bind(agent_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(to_string)?;
    let Some(agent_handle) = agent_handle else {
        return Err("agent does not exist".to_owned());
    };

    let mut resolved_channel_id = channel_id;
    let mut resolved_thread_root_id = thread_root_id;
    let mut resolved_title = title.trim().to_owned();

    if let Some(task_id) = task_id {
        let row = sqlx::query(
            r#"
            select channel_id, message_id, title
            from tasks
            where id = $1
            "#,
        )
        .bind(task_id)
        .fetch_one(&state.pool)
        .await
        .map_err(to_string)?;
        resolved_channel_id = Some(row.get("channel_id"));
        resolved_thread_root_id = Some(row.get("message_id"));
        if resolved_title.is_empty() {
            resolved_title = row.get("title");
        }
    }

    if resolved_channel_id.is_none() {
        if let Some(thread_root_id) = resolved_thread_root_id {
            resolved_channel_id =
                sqlx::query_scalar("select channel_id from messages where id = $1")
                    .bind(thread_root_id)
                    .fetch_optional(&state.pool)
                    .await
                    .map_err(to_string)?;
        }
    }

    if resolved_title.is_empty() {
        resolved_title = match resolved_thread_root_id {
            Some(thread_root_id) => {
                let body: Option<String> =
                    sqlx::query_scalar("select body from messages where id = $1")
                        .bind(thread_root_id)
                        .fetch_optional(&state.pool)
                        .await
                        .map_err(to_string)?;
                body.and_then(|body| {
                    body.lines()
                        .next()
                        .map(|line| line.chars().take(120).collect())
                })
                .filter(|line: &String| !line.trim().is_empty())
                .unwrap_or_else(|| "LocalSlock agent request".to_owned())
            }
            None => "LocalSlock agent request".to_owned(),
        };
    }

    let work_context = context.trim();
    let source_kind = if task_id.is_some() { "task" } else { "manual" };
    let work_item_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_work_items (
            agent_id, channel_id, thread_root_id, task_id, source_kind, title, context, status
        )
        values ($1, $2, $3, $4, $5, $6, $7, 'queued')
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(resolved_channel_id)
    .bind(resolved_thread_root_id)
    .bind(task_id)
    .bind(source_kind)
    .bind(&resolved_title)
    .bind(work_context)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;
    notify_ui_work_item_changed(&state.pool, work_item_id, "work_item_created").await;

    if let Some(task_id) = task_id {
        sqlx::query(
            r#"
            update tasks
            set assignee_agent_id = $2,
                status = 'in_progress',
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(task_id)
        .bind(agent_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    }

    if let (Some(channel_id), Some(thread_root_id)) = (resolved_channel_id, resolved_thread_root_id)
    {
        upsert_agent_thread_subscription(
            &state.pool,
            agent_id,
            channel_id,
            thread_root_id,
            source_kind,
            None,
        )
        .await?;
    }

    let scheduled = enqueue_agent_work_if_available(&state.pool, agent_id, work_item_id).await?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "dispatch",
        if scheduled {
            "Agent request dispatched"
        } else {
            "Agent request queued"
        },
        format!("#{work_item_id} to @{agent_handle}: {resolved_title}"),
    )
    .await?;

    Ok(work_item_id)
}

#[tauri::command]
async fn cancel_agent_work(work_item_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    let row = sqlx::query(
        r#"
        select agent_id, run_id, status
        from agent_work_items
        where id = $1
        "#,
    )
    .bind(work_item_id)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;
    let agent_id: Uuid = row.get("agent_id");
    let run_id: Option<Uuid> = row.get("run_id");
    let status: String = row.get("status");

    match status.as_str() {
        "queued" => {
            sqlx::query(
                r#"
                update agent_work_items
                set status = 'cancelled',
                    completed_at = now(),
                    updated_at = now()
                where id = $1
                "#,
            )
            .bind(work_item_id)
            .execute(&state.pool)
            .await
            .map_err(to_string)?;
            notify_ui_work_item_changed(&state.pool, work_item_id, "work_item_cancelled").await;
            sqlx::query(
                r#"
                update supervisor_commands
                set status = 'done',
                    error = 'cancelled',
                    updated_at = now()
                where work_item_id = $1 and status = 'pending'
                "#,
            )
            .bind(work_item_id)
            .execute(&state.pool)
            .await
            .map_err(to_string)?;
        }
        "running" => {
            let Some(run_id) = run_id else {
                return Err("running agent request does not have a run id".to_owned());
            };
            sqlx::query(
                r#"
                update agent_work_items
                set status = 'cancelling',
                    updated_at = now()
                where id = $1
                "#,
            )
            .bind(work_item_id)
            .execute(&state.pool)
            .await
            .map_err(to_string)?;
            notify_ui_work_item_changed(&state.pool, work_item_id, "work_item_cancelling").await;
            let pending_stop: Option<Uuid> = sqlx::query_scalar(
                r#"
                select id
                from supervisor_commands
                where command_type = 'stop_run'
                  and run_id = $1
                  and status in ('pending', 'running')
                limit 1
                "#,
            )
            .bind(run_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(to_string)?;
            if pending_stop.is_none() {
                sqlx::query(
                    r#"
                    insert into supervisor_commands (command_type, agent_id, run_id, work_item_id)
                    values ('stop_run', $1, $2, $3)
                    "#,
                )
                .bind(agent_id)
                .bind(run_id)
                .bind(work_item_id)
                .execute(&state.pool)
                .await
                .map_err(to_string)?;
                let _ = notify_supervisor_wake(&state.pool).await;
                let _ = notify_ui_refresh(&state.pool, "supervisor_command").await;
            }
        }
        "cancelling" => return Ok(()),
        other => return Err(format!("cannot cancel agent request with status {other}")),
    }

    record_agent_activity(
        &state.pool,
        Some(agent_id),
        run_id,
        "dispatch",
        "Agent request cancel requested",
        work_item_id.to_string(),
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn retry_agent_work(work_item_id: Uuid, state: State<'_, AppState>) -> CommandResult<Uuid> {
    let row = sqlx::query(
        r#"
        select agent_id, channel_id, thread_root_id, source_message_id, task_id, source_kind, title, context, status
        from agent_work_items
        where id = $1
        "#,
    )
    .bind(work_item_id)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;
    let old_status: String = row.get("status");
    if matches!(old_status.as_str(), "queued" | "running" | "cancelling") {
        return Err(format!(
            "cannot retry agent request with status {old_status}"
        ));
    }

    let agent_id: Uuid = row.get("agent_id");
    let title: String = row.get("title");
    let context: String = row.get("context");
    let new_work_item_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_work_items (
            agent_id, channel_id, thread_root_id, source_message_id, task_id, source_kind, title, context, status
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, 'queued')
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(row.get::<Option<Uuid>, _>("channel_id"))
    .bind(row.get::<Option<Uuid>, _>("thread_root_id"))
    .bind(row.get::<Option<Uuid>, _>("source_message_id"))
    .bind(row.get::<Option<Uuid>, _>("task_id"))
    .bind(row.get::<String, _>("source_kind"))
    .bind(&title)
    .bind(&context)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;
    notify_ui_work_item_changed(&state.pool, new_work_item_id, "work_item_created").await;

    let scheduled =
        enqueue_agent_work_if_available(&state.pool, agent_id, new_work_item_id).await?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "dispatch",
        if scheduled {
            "Agent request retried"
        } else {
            "Retried agent request queued"
        },
        format!("{work_item_id} -> {new_work_item_id}: {title}"),
    )
    .await?;

    Ok(new_work_item_id)
}

async fn agent_runtime(pool: &PgPool, agent_id: Uuid) -> CommandResult<Option<String>> {
    sqlx::query_scalar("select runtime from agents where id = $1")
        .bind(agent_id)
        .fetch_optional(pool)
        .await
        .map_err(to_string)
}

async fn agent_has_active_run(pool: &PgPool, agent_id: Uuid) -> CommandResult<bool> {
    let active_run: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from agent_runs
        where agent_id = $1
          and stopped_at is null
          and status in ('starting', 'running', 'stopping')
        limit 1
        "#,
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;

    Ok(active_run.is_some())
}

async fn agent_has_active_or_pending_start(pool: &PgPool, agent_id: Uuid) -> CommandResult<bool> {
    if agent_has_active_run(pool, agent_id).await? {
        return Ok(true);
    }

    let pending_start: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from supervisor_commands
        where command_type = 'start_agent'
          and agent_id = $1
          and status in ('pending', 'running')
        limit 1
        "#,
    )
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;

    Ok(pending_start.is_some())
}

async fn enqueue_agent_work_if_available(
    pool: &PgPool,
    agent_id: Uuid,
    work_item_id: Uuid,
) -> CommandResult<bool> {
    let status: Option<String> =
        sqlx::query_scalar("select status from agent_work_items where id = $1 and agent_id = $2")
            .bind(work_item_id)
            .bind(agent_id)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?;
    if status.as_deref() != Some("queued") {
        return Ok(false);
    }
    let runtime = agent_runtime(pool, agent_id).await?;
    let is_codex = runtime
        .as_deref()
        .is_some_and(|runtime| runtime.eq_ignore_ascii_case("codex"));
    let has_active_run = agent_has_active_run(pool, agent_id).await?;
    if !is_codex && agent_has_active_or_pending_start(pool, agent_id).await? {
        return Ok(false);
    }

    let pending_for_work: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from supervisor_commands
        where command_type = 'start_agent'
          and work_item_id = $1
          and status in ('pending', 'running')
        limit 1
        "#,
    )
    .bind(work_item_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;
    if pending_for_work.is_some() {
        return Ok(true);
    }

    sqlx::query(
        r#"
        insert into supervisor_commands (command_type, agent_id, work_item_id)
        values ('start_agent', $1, $2)
        "#,
    )
    .bind(agent_id)
    .bind(work_item_id)
    .execute(pool)
    .await
    .map_err(to_string)?;
    let _ = notify_supervisor_wake(pool).await;
    let _ = notify_ui_refresh(pool, "supervisor_command").await;
    sqlx::query("update agents set status = 'queued' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    if is_codex && has_active_run {
        sqlx::query("update agents set status = 'running' where id = $1")
            .bind(agent_id)
            .execute(pool)
            .await
            .map_err(to_string)?;
    }

    Ok(true)
}

fn extract_agent_mentions(body: &str) -> Vec<String> {
    let mut handles = Vec::new();
    let mut chars = body.char_indices().peekable();
    while let Some((idx, ch)) = chars.next() {
        if ch != '@' {
            continue;
        }
        if body[..idx]
            .chars()
            .next_back()
            .map(|prev| prev.is_ascii_alphanumeric() || prev == '_' || prev == '-' || prev == '.')
            .unwrap_or(false)
        {
            continue;
        }
        let mut handle = String::new();
        while let Some((_, next)) = chars.peek().copied() {
            if next.is_ascii_alphanumeric() || next == '_' || next == '-' {
                handle.push(next);
                chars.next();
            } else {
                break;
            }
        }
        if !handle.is_empty() && !handles.contains(&handle) {
            handles.push(handle);
        }
    }
    handles
}

#[derive(Clone, Copy)]
enum MentionDispatchOrigin {
    Owner,
    Agent { sender_agent_id: Uuid },
}

impl MentionDispatchOrigin {
    fn sender_agent_id(self) -> Option<Uuid> {
        match self {
            MentionDispatchOrigin::Owner => None,
            MentionDispatchOrigin::Agent { sender_agent_id } => Some(sender_agent_id),
        }
    }

    fn allows_dm_auto_dispatch(self) -> bool {
        matches!(self, MentionDispatchOrigin::Owner)
    }

    fn is_agent(self) -> bool {
        matches!(self, MentionDispatchOrigin::Agent { .. })
    }
}

const INTER_AGENT_THREAD_MESSAGE_LIMIT: i64 = 10;

#[derive(Clone, Copy, PartialEq, Eq)]
enum DispatchKind {
    Mention,
    Dm,
    ThreadFollowUp,
}

async fn upsert_agent_thread_subscription(
    pool: &PgPool,
    agent_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Uuid,
    source_kind: &str,
    source_message_id: Option<Uuid>,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        insert into agent_thread_subscriptions (
            agent_id, channel_id, thread_root_id, source_kind, last_source_message_id
        )
        values ($1, $2, $3, $4, $5)
        on conflict (agent_id, thread_root_id) do update
        set channel_id = excluded.channel_id,
            source_kind = excluded.source_kind,
            last_source_message_id = coalesce(excluded.last_source_message_id, agent_thread_subscriptions.last_source_message_id),
            updated_at = now()
        "#,
    )
    .bind(agent_id)
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(source_kind)
    .bind(source_message_id)
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

async fn queue_mentions_as_work_items(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    message_id: Uuid,
    task_id: Option<Uuid>,
    body: &str,
    origin: MentionDispatchOrigin,
) -> CommandResult<()> {
    let mentions = extract_agent_mentions(body);
    let channel_row = sqlx::query("select name, kind, dm_agent_id from channels where id = $1")
        .bind(channel_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let channel_name: String = channel_row.get("name");
    let channel_kind: String = channel_row.get("kind");
    let dm_agent_id: Option<Uuid> = channel_row.get("dm_agent_id");

    let mut targets = Vec::new();
    let mut dispatch_kind = DispatchKind::Mention;
    if channel_kind == "dm" {
        dispatch_kind = DispatchKind::Dm;
        if !origin.allows_dm_auto_dispatch() {
            return Ok(());
        }
        if let Some(agent_id) = dm_agent_id {
            let agent_handle: Option<String> =
                sqlx::query_scalar("select handle from agents where id = $1")
                    .bind(agent_id)
                    .fetch_optional(pool)
                    .await
                    .map_err(to_string)?;
            if let Some(agent_handle) = agent_handle {
                targets.push((agent_id, agent_handle));
            }
        }
    } else {
        for handle in mentions {
            let agent_id: Option<Uuid> =
                sqlx::query_scalar("select id from agents where handle = $1")
                    .bind(&handle)
                    .fetch_optional(pool)
                    .await
                    .map_err(to_string)?;
            let Some(agent_id) = agent_id else {
                continue;
            };
            if Some(agent_id) == origin.sender_agent_id() {
                continue;
            }
            if origin.is_agent() {
                let is_channel_member: bool = sqlx::query_scalar(
                    r#"
                    select exists (
                        select 1
                        from channel_members
                        where channel_id = $1 and agent_id = $2
                    )
                    "#,
                )
                .bind(channel_id)
                .bind(agent_id)
                .fetch_one(pool)
                .await
                .map_err(to_string)?;
                if !is_channel_member {
                    continue;
                }
            }
            targets.push((agent_id, handle));
        }
        if targets.is_empty() && matches!(origin, MentionDispatchOrigin::Owner) {
            if let Some(thread_root_id) = thread_root_id {
                targets = load_thread_followup_targets(pool, channel_id, thread_root_id).await?;
                if !targets.is_empty() {
                    dispatch_kind = DispatchKind::ThreadFollowUp;
                }
            }
        }
    }

    if targets.is_empty() {
        return Ok(());
    }
    let reply_thread_root_id = thread_root_id.unwrap_or(message_id);
    if origin.is_agent()
        && inter_agent_thread_message_count_since_last_owner(pool, channel_id, reply_thread_root_id)
            .await?
            >= INTER_AGENT_THREAD_MESSAGE_LIMIT
    {
        insert_system_message(
            pool,
            channel_id,
            Some(reply_thread_root_id),
            format!(
                "Inter-agent collaboration paused: this thread reached {INTER_AGENT_THREAD_MESSAGE_LIMIT} agent messages. Add a human reply to continue."
            ),
        )
        .await?;
        return Ok(());
    }

    let title = body
        .lines()
        .next()
        .map(|line| line.chars().take(120).collect::<String>())
        .filter(|line| !line.trim().is_empty())
        .unwrap_or_else(|| match dispatch_kind {
            DispatchKind::Dm => format!("DM in #{channel_name}"),
            DispatchKind::Mention => format!("Mention in #{channel_name}"),
            DispatchKind::ThreadFollowUp => format!("Thread follow-up in #{channel_name}"),
        });

    for (agent_id, agent_handle) in targets {
        let already_queued: bool = sqlx::query_scalar(
            r#"
            select exists (
                select 1
                from agent_work_items
                where source_message_id = $1 and agent_id = $2
            )
            "#,
        )
        .bind(message_id)
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
        if already_queued {
            continue;
        }

        sqlx::query(
            r#"
            insert into channel_members (channel_id, agent_id)
            values ($1, $2)
            on conflict (channel_id, agent_id) do nothing
            "#,
        )
        .bind(channel_id)
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;

        let channel_label = if channel_kind == "dm" {
            format!("DM with @{agent_handle}")
        } else {
            format!("#{channel_name}")
        };
        let (message_id_label, message_body_label) = match dispatch_kind {
            DispatchKind::ThreadFollowUp => ("Thread reply message id", "Latest thread reply"),
            DispatchKind::Dm | DispatchKind::Mention => {
                ("Mentioned message id", "Mentioned message")
            }
        };
        let source_kind = if task_id.is_some() {
            "task"
        } else {
            match (dispatch_kind, origin.is_agent()) {
                (DispatchKind::Dm, _) => "dm",
                (DispatchKind::ThreadFollowUp, _) => "thread_followup",
                (DispatchKind::Mention, true) => "collaboration",
                (DispatchKind::Mention, false) => "mention",
            }
        };
        upsert_agent_thread_subscription(
            pool,
            agent_id,
            channel_id,
            reply_thread_root_id,
            source_kind,
            Some(message_id),
        )
        .await?;
        let context = build_dispatch_work_context(
            pool,
            channel_id,
            &channel_label,
            Some(reply_thread_root_id),
            message_id,
            body,
            message_id_label,
            message_body_label,
        )
        .await?;
        let work_item_id: Uuid = sqlx::query_scalar(
            r#"
            insert into agent_work_items (
                agent_id, channel_id, thread_root_id, source_message_id, task_id, source_kind, title, context, status
            )
            values ($1, $2, $3, $4, $5, $6, $7, $8, 'queued')
            returning id
            "#,
        )
        .bind(agent_id)
        .bind(channel_id)
        .bind(reply_thread_root_id)
        .bind(message_id)
        .bind(task_id)
        .bind(source_kind)
        .bind(&title)
        .bind(&context)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_created").await;
        let scheduled = enqueue_agent_work_if_available(pool, agent_id, work_item_id).await?;
        record_agent_activity(
            pool,
            Some(agent_id),
            None,
            match dispatch_kind {
                DispatchKind::Dm => "dm",
                DispatchKind::Mention => "mention",
                DispatchKind::ThreadFollowUp => "thread",
            },
            match (dispatch_kind, scheduled, origin.is_agent()) {
                (DispatchKind::Dm, true, _) => "DM dispatched",
                (DispatchKind::Dm, false, _) => "DM queued",
                (DispatchKind::ThreadFollowUp, true, _) => "Thread follow-up dispatched",
                (DispatchKind::ThreadFollowUp, false, _) => "Thread follow-up queued",
                (DispatchKind::Mention, true, true) => "Collaboration dispatched",
                (DispatchKind::Mention, false, true) => "Collaboration queued",
                (DispatchKind::Mention, true, false) => "Mention dispatched",
                (DispatchKind::Mention, false, false) => "Mention queued",
            },
            format!("#{channel_name} to @{agent_handle}: {title}"),
        )
        .await?;
    }

    Ok(())
}

async fn load_thread_followup_targets(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Uuid,
) -> CommandResult<Vec<(Uuid, String)>> {
    let rows = sqlx::query(
        r#"
        select a.id, a.handle
        from (
            select agent_id, max(last_at) as last_at
            from (
                select sender_agent_id as agent_id, max(created_at) as last_at
                from messages
                where channel_id = $1
                  and (id = $2 or thread_root_id = $2)
                  and sender_agent_id is not null
                group by sender_agent_id
                union all
                select agent_id, max(created_at) as last_at
                from agent_work_items
                where channel_id = $1
                  and thread_root_id = $2
                group by agent_id
                union all
                select agent_id, max(updated_at) as last_at
                from agent_thread_subscriptions
                where channel_id = $1
                  and thread_root_id = $2
                group by agent_id
            ) candidates
            where agent_id is not null
            group by agent_id
        ) candidates
        join agents a on a.id = candidates.agent_id
        join channel_members cm on cm.channel_id = $1 and cm.agent_id = a.id
        order by candidates.last_at desc, lower(a.handle)
        limit 8
        "#,
    )
    .bind(channel_id)
    .bind(thread_root_id)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| (row.get("id"), row.get("handle")))
        .collect())
}

async fn inter_agent_thread_message_count_since_last_owner(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Uuid,
) -> CommandResult<i64> {
    let last_owner_created_at: Option<DateTime<Utc>> = sqlx::query_scalar(
        r#"
        select max(created_at)
        from messages
        where channel_id = $1
          and (id = $2 or thread_root_id = $2)
          and sender_role = 'owner'
        "#,
    )
    .bind(channel_id)
    .bind(thread_root_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let count = if let Some(last_owner_created_at) = last_owner_created_at {
        sqlx::query_scalar(
            r#"
            select count(*)::bigint
            from messages
            where channel_id = $1
              and (id = $2 or thread_root_id = $2)
              and sender_agent_id is not null
              and created_at > $3
            "#,
        )
        .bind(channel_id)
        .bind(thread_root_id)
        .bind(last_owner_created_at)
        .fetch_one(pool)
        .await
        .map_err(to_string)?
    } else {
        sqlx::query_scalar(
            r#"
            select count(*)::bigint
            from messages
            where channel_id = $1
              and (id = $2 or thread_root_id = $2)
              and sender_agent_id is not null
            "#,
        )
        .bind(channel_id)
        .bind(thread_root_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?
    };
    Ok(count)
}

async fn queue_agent_message_mentions(pool: &PgPool, message_id: Uuid) -> CommandResult<()> {
    let Some(row) = sqlx::query(
        r#"
        select channel_id, thread_root_id, sender_agent_id, body, is_task
        from messages
        where id = $1
        "#,
    )
    .bind(message_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?
    else {
        return Ok(());
    };

    let Some(sender_agent_id) = row.get::<Option<Uuid>, _>("sender_agent_id") else {
        return Ok(());
    };
    let is_task: bool = row.get("is_task");
    if is_task {
        return Ok(());
    }

    let channel_id: Uuid = row.get("channel_id");
    let thread_root_id: Option<Uuid> = row.get("thread_root_id");
    let body: String = row.get("body");
    queue_mentions_as_work_items(
        pool,
        channel_id,
        thread_root_id,
        message_id,
        None,
        body.trim(),
        MentionDispatchOrigin::Agent { sender_agent_id },
    )
    .await
}

async fn build_dispatch_work_context(
    pool: &PgPool,
    channel_id: Uuid,
    channel_label: &str,
    thread_root_id: Option<Uuid>,
    message_id: Uuid,
    body: &str,
    message_id_label: &str,
    message_body_label: &str,
) -> CommandResult<String> {
    let mut lines = vec![
        format!("Surface: {channel_label}"),
        format!("{message_id_label}: {message_id}"),
    ];
    if let Some(thread_root_id) = thread_root_id {
        lines.push(format!("Thread root id: {thread_root_id}"));
    }
    lines.push(format!("{message_body_label}:"));
    lines.push(compact_chars_middle(body, DISPATCH_MESSAGE_BODY_LIMIT));
    let latest_attachments = load_message_attachment_lines(pool, &[message_id]).await?;
    if let Some(attachments) = latest_attachments.get(&message_id) {
        lines.push("Latest message attachments:".to_owned());
        lines.extend(attachments.iter().cloned());
        lines.push(
            "If an attachment is an image, inspect the local_path directly with your runtime's file/vision support before answering UI-specific questions."
                .to_owned(),
        );
    }

    if let Some(thread_root_id) = thread_root_id {
        let rows = sqlx::query(
            r#"
            select id, sender_name, sender_role, body, created_at
            from messages
            where id = $1 or thread_root_id = $1
            order by created_at desc
            limit $2
            "#,
        )
        .bind(thread_root_id)
        .bind(DISPATCH_THREAD_MESSAGE_LIMIT)
        .fetch_all(pool)
        .await
        .map_err(to_string)?;

        if !rows.is_empty() {
            let message_ids = rows
                .iter()
                .map(|row| row.get::<Uuid, _>("id"))
                .collect::<Vec<_>>();
            let attachments_by_message = load_message_attachment_lines(pool, &message_ids).await?;
            lines.push("Recent thread context, oldest first:".to_owned());
            for row in rows.into_iter().rev() {
                let id: Uuid = row.get("id");
                let sender_name: String = row.get("sender_name");
                let sender_role: String = row.get("sender_role");
                let created_at: DateTime<Utc> = row.get("created_at");
                let body: String = row.get("body");
                let body = compact_chars_middle(&body, DISPATCH_THREAD_MESSAGE_BODY_LIMIT);
                lines.push(format!(
                    "- {sender_name} ({sender_role}) at {}: {}",
                    created_at.to_rfc3339(),
                    body.replace('\n', "\n  ")
                ));
                if let Some(attachments) = attachments_by_message.get(&id) {
                    for attachment in attachments {
                        lines.push(format!("  {attachment}"));
                    }
                }
            }
        }
    }

    if let Some(thread_root_id) = thread_root_id {
        lines.push(format!(
            "Reply in the same thread with: LOCAL_SLOCK_EVENT {{\"type\":\"message\",\"channel_id\":\"{channel_id}\",\"thread_root_id\":\"{thread_root_id}\",\"body\":\"...\"}}"
        ));
    } else {
        lines.push(format!(
            "Reply in the channel with: LOCAL_SLOCK_EVENT {{\"type\":\"message\",\"channel_id\":\"{channel_id}\",\"body\":\"...\"}}"
        ));
    }
    lines.push(
        "Use normal stdout for private logs; only LOCAL_SLOCK_EVENT creates visible chat messages."
            .to_owned(),
    );

    Ok(compact_chars_middle(
        &lines.join("\n\n"),
        DISPATCH_CONTEXT_LIMIT,
    ))
}

#[tauri::command]
async fn stop_agent(run_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    let row = sqlx::query(
        r#"
        select agent_id, pid, work_item_id
        from agent_runs
        where id = $1 and stopped_at is null
        "#,
    )
    .bind(run_id)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;

    let agent_id: Uuid = row.get("agent_id");
    sqlx::query(
        r#"
        insert into supervisor_commands (command_type, agent_id, run_id)
        values ('stop_run', $1, $2)
        "#,
    )
    .bind(agent_id)
    .bind(run_id)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;
    let _ = notify_supervisor_wake(&state.pool).await;
    let _ = notify_ui_refresh(&state.pool, "supervisor_command").await;
    sqlx::query("update agent_runs set status = 'stopping' where id = $1")
        .bind(run_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    notify_ui_agent_run_changed(&state.pool, run_id, "run_stopping").await;
    sqlx::query("update agents set status = 'stopping' where id = $1")
        .bind(agent_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Stop requested",
        "Stop command queued for supervisor",
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn install_supervisor_service(
    state: State<'_, AppState>,
) -> CommandResult<LaunchAgentStatus> {
    let plist_path = launch_agent_plist_path()?;
    let exe_path = env::current_exe().map_err(to_string)?;
    let plist = render_launch_agent_plist(&exe_path, &state.db_url);

    if let Some(parent) = plist_path.parent() {
        fs::create_dir_all(parent).map_err(to_string)?;
    }
    fs::write(&plist_path, plist).map_err(to_string)?;

    let domain = launch_agent_domain()?;
    let service = launch_agent_service_target(&domain);
    let _ = StdCommand::new("launchctl")
        .arg("bootout")
        .arg(&service)
        .output();

    run_launchctl(&["bootstrap", &domain, &plist_path.to_string_lossy()])?;
    run_launchctl(&["kickstart", "-k", &service])?;

    load_launch_agent_status()
}

#[tauri::command]
async fn uninstall_supervisor_service(
    state: State<'_, AppState>,
) -> CommandResult<LaunchAgentStatus> {
    let domain = launch_agent_domain()?;
    let service = launch_agent_service_target(&domain);
    let _ = StdCommand::new("launchctl")
        .arg("bootout")
        .arg(&service)
        .output();

    let plist_path = launch_agent_plist_path()?;
    match fs::remove_file(&plist_path) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err.to_string()),
    }

    sqlx::query("update supervisor_state set status = 'offline', updated_at = now() where id = 1")
        .execute(&state.pool)
        .await
        .map_err(to_string)?;

    load_launch_agent_status()
}

#[tauri::command]
async fn send_message(
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    body: String,
    as_task: bool,
    attachments: Option<Vec<AttachmentUpload>>,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    send_owner_message_in_pool(
        &state.pool,
        channel_id,
        thread_root_id,
        &body,
        as_task,
        attachments.unwrap_or_default(),
    )
    .await
}

async fn send_owner_message_in_pool(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    body: &str,
    as_task: bool,
    attachments: Vec<AttachmentUpload>,
) -> CommandResult<()> {
    if body.trim().is_empty() && attachments.is_empty() {
        return Err("message body or attachment is required".to_owned());
    }
    let mut tx = pool.begin().await.map_err(to_string)?;
    let channel_kind: Option<String> =
        sqlx::query_scalar("select kind from channels where id = $1")
            .bind(channel_id)
            .fetch_optional(&mut *tx)
            .await
            .map_err(to_string)?;
    let Some(channel_kind) = channel_kind else {
        return Err("channel does not exist".to_owned());
    };
    if as_task && channel_kind == "dm" {
        return Err("direct messages do not support tasks".to_owned());
    }

    let msg_id: Uuid = sqlx::query_scalar(
        r#"
        insert into messages (channel_id, thread_root_id, sender_name, sender_role, body, is_task)
        values ($1, $2, 'Dylan', 'owner', $3, $4)
        returning id
        "#,
    )
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(body.trim())
    .bind(as_task)
    .fetch_one(&mut *tx)
    .await
    .map_err(to_string)?;

    for attachment in attachments {
        if attachment.bytes.is_empty() {
            continue;
        }
        if attachment.bytes.len() > ATTACHMENT_SIZE_LIMIT {
            return Err(format!(
                "attachment {} is larger than 25MB",
                attachment.original_name
            ));
        }
        let attachment_id = Uuid::new_v4();
        let original_name = attachment.original_name.trim();
        let original_name = if original_name.is_empty() {
            "attachment"
        } else {
            original_name
        };
        let mime_type = attachment.mime_type.trim();
        let mime_type = if mime_type.is_empty() {
            "application/octet-stream"
        } else {
            mime_type
        };
        let storage_path =
            write_attachment_file(msg_id, attachment_id, original_name, &attachment.bytes)?;
        sqlx::query(
            r#"
            insert into message_attachments (
                id,
                message_id,
                original_name,
                mime_type,
                size_bytes,
                storage_path
            )
            values ($1, $2, $3, $4, $5, $6)
            "#,
        )
        .bind(attachment_id)
        .bind(msg_id)
        .bind(original_name)
        .bind(mime_type)
        .bind(attachment.bytes.len() as i64)
        .bind(storage_path)
        .execute(&mut *tx)
        .await
        .map_err(to_string)?;
    }

    let mut task_id = None;
    if as_task {
        task_id = Some(
            sqlx::query_scalar(
                r#"
            insert into tasks (message_id, channel_id, title, status)
            values ($1, $2, $3, 'todo')
            returning id
            "#,
            )
            .bind(msg_id)
            .bind(channel_id)
            .bind(body.lines().next().unwrap_or("Untitled task"))
            .fetch_one(&mut *tx)
            .await
            .map_err(to_string)?,
        );
    }

    tx.commit().await.map_err(to_string)?;
    queue_mentions_as_work_items(
        pool,
        channel_id,
        thread_root_id,
        msg_id,
        task_id,
        body.trim(),
        MentionDispatchOrigin::Owner,
    )
    .await?;
    let _ = notify_ui_refresh(pool, "message").await;
    Ok(())
}

#[tauri::command]
async fn update_message(
    message_id: Uuid,
    body: String,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let body = body.trim();
    if body.is_empty() {
        return Err("message body is empty".to_owned());
    }

    let mut tx = state.pool.begin().await.map_err(to_string)?;
    let result = sqlx::query("update messages set body = $2, updated_at = now() where id = $1")
        .bind(message_id)
        .bind(body)
        .execute(&mut *tx)
        .await
        .map_err(to_string)?;
    if result.rows_affected() == 0 {
        return Err("message does not exist".to_owned());
    }

    sqlx::query(
        r#"
        update tasks
        set title = $2, updated_at = now()
        where message_id = $1
        "#,
    )
    .bind(message_id)
    .bind(body.lines().next().unwrap_or("Untitled task"))
    .execute(&mut *tx)
    .await
    .map_err(to_string)?;

    tx.commit().await.map_err(to_string)?;
    Ok(())
}

#[tauri::command]
async fn delete_message(message_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    let result = sqlx::query("delete from messages where id = $1")
        .bind(message_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    if result.rows_affected() == 0 {
        return Err("message does not exist".to_owned());
    }
    Ok(())
}

#[tauri::command]
async fn claim_task(
    task_id: Uuid,
    agent_id: Option<Uuid>,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let row = sqlx::query(
        r#"
        with updated as (
            update tasks
            set assignee_agent_id = $2,
                status = case when $2 is null then status else 'in_progress' end,
                updated_at = now()
            where id = $1
            returning channel_id, number, title, assignee_agent_id
        )
        select
            u.channel_id,
            u.number,
            u.title,
            a.handle as assignee_handle
        from updated u
        left join agents a on a.id = u.assignee_agent_id
        "#,
    )
    .bind(task_id)
    .bind(agent_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(to_string)?
    .ok_or_else(|| "task does not exist".to_owned())?;

    let channel_id: Uuid = row.get("channel_id");
    let number: i64 = row.get("number");
    let title: String = row.get("title");
    let body = if let Some(handle) = row.get::<Option<String>, _>("assignee_handle") {
        format!("@{handle} claimed task #{number}: {title}")
    } else {
        format!("Task #{number} was unassigned: {title}")
    };
    insert_system_message(&state.pool, channel_id, None, body).await?;

    Ok(())
}

#[tauri::command]
async fn update_task_status(
    task_id: Uuid,
    status: String,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let status = status.trim();
    if !matches!(status, "todo" | "in_progress" | "in_review" | "done") {
        return Err(format!("unsupported task status: {status}"));
    }

    let row = sqlx::query(
        r#"
        update tasks
        set status = $2, updated_at = now()
        where id = $1
        returning channel_id, number, title
        "#,
    )
    .bind(task_id)
    .bind(status)
    .fetch_optional(&state.pool)
    .await
    .map_err(to_string)?
    .ok_or_else(|| "task does not exist".to_owned())?;

    let channel_id: Uuid = row.get("channel_id");
    let number: i64 = row.get("number");
    let title: String = row.get("title");
    insert_system_message(
        &state.pool,
        channel_id,
        None,
        format!("Task #{number} moved to {}: {title}", status_label(status)),
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn update_task_title(
    task_id: Uuid,
    title: String,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let title = title.trim();
    if title.is_empty() {
        return Err("task title is empty".to_owned());
    }

    let mut tx = state.pool.begin().await.map_err(to_string)?;
    let message_id: Uuid = sqlx::query_scalar(
        r#"
        update tasks
        set title = $2, updated_at = now()
        where id = $1
        returning message_id
        "#,
    )
    .bind(task_id)
    .bind(title)
    .fetch_one(&mut *tx)
    .await
    .map_err(to_string)?;

    sqlx::query("update messages set body = $2, updated_at = now() where id = $1")
        .bind(message_id)
        .bind(title)
        .execute(&mut *tx)
        .await
        .map_err(to_string)?;

    tx.commit().await.map_err(to_string)?;
    Ok(())
}

fn parse_due_at(value: &str) -> CommandResult<DateTime<Utc>> {
    DateTime::parse_from_rfc3339(value)
        .map(|value| value.with_timezone(&Utc))
        .map_err(|err| format!("invalid reminder due_at: {err}"))
}

fn normalize_recurrence(value: &str) -> CommandResult<String> {
    let recurrence = value.trim();
    if matches!(recurrence, "none" | "daily" | "weekly") {
        Ok(recurrence.to_owned())
    } else if matches!(recurrence, "one_shot" | "one-shot" | "once") {
        Ok("none".to_owned())
    } else {
        Err(format!("unsupported reminder recurrence: {recurrence}"))
    }
}

fn normalize_schedule_cadence(value: &str) -> CommandResult<String> {
    let cadence = value.trim();
    if matches!(cadence, "hourly" | "daily" | "weekly") {
        Ok(cadence.to_owned())
    } else {
        Err(format!("unsupported schedule cadence: {cadence}"))
    }
}

fn build_schedule_work_context(
    schedule_id: Uuid,
    agent_handle: &str,
    channel_name: &str,
    thread_root_id: Option<Uuid>,
    source_message_id: Uuid,
    title: &str,
    prompt: &str,
    cadence: &str,
) -> String {
    let mut lines = vec![
        format!("Scheduled routine id: {schedule_id}"),
        format!("Assigned agent: @{agent_handle}"),
        format!("Surface: #{channel_name}"),
        format!("Source message id: {source_message_id}"),
        format!("Cadence: {cadence}"),
    ];
    if let Some(thread_root_id) = thread_root_id {
        lines.push(format!("Thread root id: {thread_root_id}"));
    }
    lines.push(format!("Routine title: {title}"));
    lines.push("Routine prompt:".to_owned());
    lines.push(prompt.trim().to_owned());
    lines.join("\n")
}

async fn insert_reminder_event(
    pool: &PgPool,
    reminder_id: Uuid,
    event_type: &str,
    detail: impl AsRef<str>,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        insert into reminder_events (reminder_id, event_type, detail)
        values ($1, $2, $3)
        "#,
    )
    .bind(reminder_id)
    .bind(event_type)
    .bind(detail.as_ref())
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

async fn create_reminder_in_pool(
    pool: &PgPool,
    creator_agent_id: Option<Uuid>,
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
    message_id: Option<Uuid>,
    title: &str,
    note: &str,
    due_at: DateTime<Utc>,
    recurrence: &str,
) -> CommandResult<Uuid> {
    let title = title.trim();
    if title.is_empty() {
        return Err("reminder title is empty".to_owned());
    }
    let recurrence = normalize_recurrence(recurrence)?;

    if let Some(thread_root_id) = thread_root_id {
        let exists: bool = sqlx::query_scalar(
            "select exists(select 1 from messages where id = $1 and thread_root_id is null)",
        )
        .bind(thread_root_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
        if !exists {
            return Err("thread root does not exist".to_owned());
        }
    }

    let reminder_id: Uuid = sqlx::query_scalar(
        r#"
        insert into reminders (
            channel_id, creator_agent_id, thread_root_id, message_id, title, note, due_at, recurrence, status
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, 'scheduled')
        returning id
        "#,
    )
    .bind(channel_id)
    .bind(creator_agent_id)
    .bind(thread_root_id)
    .bind(message_id)
    .bind(title)
    .bind(note.trim())
    .bind(due_at)
    .bind(&recurrence)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    insert_reminder_event(pool, reminder_id, "created", due_at.to_rfc3339()).await?;
    let _ = notify_ui_refresh(pool, "reminder_created").await;
    Ok(reminder_id)
}

async fn cancel_reminder_in_pool(pool: &PgPool, reminder_id: Uuid) -> CommandResult<()> {
    let affected = sqlx::query(
        r#"
        update reminders
        set status = 'cancelled',
            completed_at = now(),
            updated_at = now()
        where id = $1 and status in ('scheduled', 'fired')
        "#,
    )
    .bind(reminder_id)
    .execute(pool)
    .await
    .map_err(to_string)?
    .rows_affected();
    if affected == 0 {
        return Err("reminder does not exist or is not active".to_owned());
    }
    insert_reminder_event(pool, reminder_id, "cancelled", "").await?;
    let _ = notify_ui_refresh(pool, "reminder_cancelled").await;
    Ok(())
}

#[tauri::command]
async fn create_reminder(
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
    message_id: Option<Uuid>,
    title: String,
    note: String,
    due_at: String,
    recurrence: String,
    state: State<'_, AppState>,
) -> CommandResult<Uuid> {
    let due_at = parse_due_at(&due_at)?;
    create_reminder_in_pool(
        &state.pool,
        None,
        channel_id,
        thread_root_id,
        message_id,
        &title,
        &note,
        due_at,
        &recurrence,
    )
    .await
}

#[tauri::command]
async fn snooze_reminder(
    reminder_id: Uuid,
    minutes: i64,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    if !(1..=10_080).contains(&minutes) {
        return Err("snooze minutes must be between 1 and 10080".to_owned());
    }
    sqlx::query(
        r#"
        update reminders
        set status = 'scheduled',
            due_at = now() + ($2::text || ' minutes')::interval,
            updated_at = now()
        where id = $1 and status in ('scheduled', 'fired')
        "#,
    )
    .bind(reminder_id)
    .bind(minutes)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;
    insert_reminder_event(
        &state.pool,
        reminder_id,
        "snoozed",
        format!("{minutes} minutes"),
    )
    .await?;
    let _ = notify_ui_refresh(&state.pool, "reminder_snoozed").await;
    Ok(())
}

#[tauri::command]
async fn complete_reminder(reminder_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    sqlx::query(
        r#"
        update reminders
        set status = 'done',
            completed_at = now(),
            updated_at = now()
        where id = $1 and status in ('scheduled', 'fired')
        "#,
    )
    .bind(reminder_id)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;
    insert_reminder_event(&state.pool, reminder_id, "completed", "").await?;
    let _ = notify_ui_refresh(&state.pool, "reminder_completed").await;
    Ok(())
}

#[tauri::command]
async fn cancel_reminder(reminder_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    cancel_reminder_in_pool(&state.pool, reminder_id).await
}

#[tauri::command]
async fn create_agent_schedule(
    agent_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    title: String,
    prompt: String,
    cadence: String,
    next_run_at: String,
    state: State<'_, AppState>,
) -> CommandResult<Uuid> {
    let title = title.trim();
    let prompt = prompt.trim();
    if title.is_empty() {
        return Err("schedule title is empty".to_owned());
    }
    if prompt.is_empty() {
        return Err("schedule prompt is empty".to_owned());
    }
    let next_run_at = parse_due_at(&next_run_at)?;
    let cadence = normalize_schedule_cadence(&cadence)?;

    let agent_exists: bool =
        sqlx::query_scalar("select exists(select 1 from agents where id = $1)")
            .bind(agent_id)
            .fetch_one(&state.pool)
            .await
            .map_err(to_string)?;
    if !agent_exists {
        return Err("agent does not exist".to_owned());
    }

    let channel_row = sqlx::query("select kind, dm_agent_id from channels where id = $1")
        .bind(channel_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(to_string)?;
    let Some(channel_row) = channel_row else {
        return Err("channel does not exist".to_owned());
    };
    let channel_kind: String = channel_row.get("kind");
    let dm_agent_id: Option<Uuid> = channel_row.get("dm_agent_id");
    if channel_kind == "dm" && dm_agent_id != Some(agent_id) {
        return Err("direct message schedules must target their DM agent".to_owned());
    }

    if let Some(thread_root_id) = thread_root_id {
        let root_channel: Option<Uuid> = sqlx::query_scalar(
            "select channel_id from messages where id = $1 and thread_root_id is null",
        )
        .bind(thread_root_id)
        .fetch_optional(&state.pool)
        .await
        .map_err(to_string)?;
        if root_channel != Some(channel_id) {
            return Err("thread root does not belong to target channel".to_owned());
        }
    }

    if channel_kind != "dm" {
        sqlx::query(
            r#"
            insert into channel_members (channel_id, agent_id)
            values ($1, $2)
            on conflict (channel_id, agent_id) do nothing
            "#,
        )
        .bind(channel_id)
        .bind(agent_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
    }

    let schedule_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_schedules (
            agent_id, channel_id, thread_root_id, title, prompt, cadence, status, next_run_at
        )
        values ($1, $2, $3, $4, $5, $6, 'active', $7)
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(title)
    .bind(prompt)
    .bind(&cadence)
    .bind(next_run_at)
    .fetch_one(&state.pool)
    .await
    .map_err(to_string)?;

    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "schedule",
        "Scheduled routine created",
        json!({
            "schedule_id": schedule_id,
            "cadence": cadence,
            "next_run_at": next_run_at.to_rfc3339()
        })
        .to_string(),
    )
    .await?;
    let _ = notify_ui_refresh(&state.pool, "agent_schedule_created").await;
    Ok(schedule_id)
}

#[tauri::command]
async fn update_agent_schedule_status(
    schedule_id: Uuid,
    status: String,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    let status = status.trim();
    if !matches!(status, "active" | "paused" | "cancelled") {
        return Err(format!("unsupported schedule status: {status}"));
    }
    let row = sqlx::query(
        r#"
        update agent_schedules
        set status = $2,
            updated_at = now()
        where id = $1 and status <> 'cancelled'
        returning agent_id
        "#,
    )
    .bind(schedule_id)
    .bind(status)
    .fetch_optional(&state.pool)
    .await
    .map_err(to_string)?;
    let Some(row) = row else {
        return Err("schedule does not exist or is already cancelled".to_owned());
    };
    let agent_id: Uuid = row.get("agent_id");
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "schedule",
        match status {
            "active" => "Scheduled routine resumed",
            "paused" => "Scheduled routine paused",
            "cancelled" => "Scheduled routine cancelled",
            _ => "Scheduled routine updated",
        },
        schedule_id.to_string(),
    )
    .await?;
    let _ = notify_ui_refresh(&state.pool, "agent_schedule_updated").await;
    Ok(())
}

#[tauri::command]
async fn mark_channel_read(channel_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    sqlx::query(
        r#"
        insert into channel_read_state (channel_id, last_read_at)
        values ($1, now())
        on conflict (channel_id) do update set last_read_at = excluded.last_read_at
        "#,
    )
    .bind(channel_id)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

#[tauri::command]
async fn update_thread_followed(
    thread_root_id: Uuid,
    followed: bool,
    state: State<'_, AppState>,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        update messages
        set thread_followed = $2
        where id = $1 and thread_root_id is null
        "#,
    )
    .bind(thread_root_id)
    .bind(followed)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

async fn load_channels(pool: &PgPool) -> CommandResult<Vec<Channel>> {
    let rows = sqlx::query(
        r#"
        select
            c.id,
            c.name,
            c.description,
            c.kind,
            c.dm_agent_id,
            count(m.id) filter (
                where m.created_at > coalesce(r.last_read_at, '-infinity'::timestamptz)
            )::integer as unread_count
        from channels c
        left join channel_read_state r on r.channel_id = c.id
        left join messages m on m.channel_id = c.id
        group by c.id, c.name, c.description, c.kind, c.dm_agent_id
        order by
          case
            when c.kind = 'channel' and c.name = 'local-slock' then 0
            when c.kind = 'channel' then 1
            else 2
          end,
          c.name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| Channel {
            id: row.get("id"),
            name: row.get("name"),
            description: row.get("description"),
            kind: row.get("kind"),
            dm_agent_id: row.get("dm_agent_id"),
            unread_count: row.get("unread_count"),
        })
        .collect())
}

async fn load_channel_members(pool: &PgPool) -> CommandResult<Vec<ChannelMember>> {
    let rows = sqlx::query(
        r#"
        select
            m.channel_id,
            m.agent_id,
            a.handle as agent_handle,
            a.display_name as agent_display_name,
            m.created_at
        from channel_members m
        join agents a on a.id = m.agent_id
        join channels c on c.id = m.channel_id
        order by c.name, a.handle
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| ChannelMember {
            channel_id: row.get("channel_id"),
            agent_id: row.get("agent_id"),
            agent_handle: row.get("agent_handle"),
            agent_display_name: row.get("agent_display_name"),
            created_at: row.get("created_at"),
        })
        .collect())
}

async fn load_agents(pool: &PgPool) -> CommandResult<Vec<Agent>> {
    let rows = sqlx::query(
        r#"
        select
            id,
            handle,
            display_name,
            role,
            status,
            runtime,
            model,
            avatar,
            description,
            launch_command,
            working_directory,
            daily_budget_micros
        from agents
        order by case when handle = 'Hancock' then 0 else 1 end, display_name
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| Agent {
            id: row.get("id"),
            handle: row.get("handle"),
            display_name: row.get("display_name"),
            role: row.get("role"),
            status: row.get("status"),
            runtime: row.get("runtime"),
            model: row.get("model"),
            avatar: row.get("avatar"),
            description: row.get("description"),
            launch_command: row.get("launch_command"),
            working_directory: row.get("working_directory"),
            daily_budget_micros: row.get("daily_budget_micros"),
        })
        .collect())
}

async fn load_messages(pool: &PgPool) -> CommandResult<Vec<Message>> {
    let rows = sqlx::query(
        r#"
        select
            m.id,
            m.channel_id,
            m.thread_root_id,
            m.sender_name,
            m.sender_role,
            m.body,
            m.is_task,
            m.thread_followed,
            m.delivery_state,
            m.stream_key,
            t.number as task_number,
            t.status as task_status,
            m.created_at,
            m.updated_at
        from messages m
        left join tasks t on t.message_id = m.id
        order by m.created_at asc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    let mut messages: Vec<Message> = rows
        .into_iter()
        .map(|row| Message {
            id: row.get("id"),
            channel_id: row.get("channel_id"),
            thread_root_id: row.get("thread_root_id"),
            sender_name: row.get("sender_name"),
            sender_role: row.get("sender_role"),
            body: row.get("body"),
            is_task: row.get("is_task"),
            thread_followed: row.get("thread_followed"),
            delivery_state: row.get("delivery_state"),
            stream_key: row.get("stream_key"),
            task_number: row.get("task_number"),
            task_status: row.get("task_status"),
            attachments: Vec::new(),
            artifacts: Vec::new(),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect();
    attach_message_attachments(pool, &mut messages).await?;
    attach_message_artifacts(pool, &mut messages).await?;
    Ok(messages)
}

async fn load_message(pool: &PgPool, message_id: Uuid) -> CommandResult<Message> {
    let row = sqlx::query(
        r#"
        select
            m.id,
            m.channel_id,
            m.thread_root_id,
            m.sender_name,
            m.sender_role,
            m.body,
            m.is_task,
            m.thread_followed,
            m.delivery_state,
            m.stream_key,
            t.number as task_number,
            t.status as task_status,
            m.created_at,
            m.updated_at
        from messages m
        left join tasks t on t.message_id = m.id
        where m.id = $1
        "#,
    )
    .bind(message_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let mut message = Message {
        id: row.get("id"),
        channel_id: row.get("channel_id"),
        thread_root_id: row.get("thread_root_id"),
        sender_name: row.get("sender_name"),
        sender_role: row.get("sender_role"),
        body: row.get("body"),
        is_task: row.get("is_task"),
        thread_followed: row.get("thread_followed"),
        delivery_state: row.get("delivery_state"),
        stream_key: row.get("stream_key"),
        task_number: row.get("task_number"),
        task_status: row.get("task_status"),
        attachments: Vec::new(),
        artifacts: Vec::new(),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    };
    attach_message_attachments(pool, std::slice::from_mut(&mut message)).await?;
    attach_message_artifacts(pool, std::slice::from_mut(&mut message)).await?;
    Ok(message)
}

async fn attach_message_attachments(pool: &PgPool, messages: &mut [Message]) -> CommandResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = messages.iter().map(|message| message.id).collect();
    let rows = sqlx::query(
        r#"
        select id, message_id, original_name, mime_type, size_bytes, storage_path, created_at
        from message_attachments
        where message_id = any($1)
        order by created_at asc
        "#,
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    let mut attachments_by_message: HashMap<Uuid, Vec<MessageAttachment>> = HashMap::new();
    for row in rows {
        let attachment = MessageAttachment {
            id: row.get("id"),
            message_id: row.get("message_id"),
            original_name: row.get("original_name"),
            mime_type: row.get("mime_type"),
            size_bytes: row.get("size_bytes"),
            storage_path: row.get("storage_path"),
            created_at: row.get("created_at"),
        };
        attachments_by_message
            .entry(attachment.message_id)
            .or_default()
            .push(attachment);
    }
    for message in messages {
        message.attachments = attachments_by_message
            .remove(&message.id)
            .unwrap_or_default();
    }
    Ok(())
}

fn artifact_from_row(row: &sqlx::postgres::PgRow) -> Artifact {
    Artifact {
        id: row.get("id"),
        message_id: row.get("message_id"),
        channel_id: row.get("channel_id"),
        thread_root_id: row.get("thread_root_id"),
        creator_agent_id: row.get("creator_agent_id"),
        creator_agent_handle: row.get("creator_agent_handle"),
        kind: row.get("kind"),
        title: row.get("title"),
        summary: row.get("summary"),
        content: row.get("content"),
        metadata: row.get("metadata"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
    }
}

async fn load_artifacts(pool: &PgPool) -> CommandResult<Vec<Artifact>> {
    let rows = sqlx::query(
        r#"
        select
            ar.id,
            ar.message_id,
            ar.channel_id,
            ar.thread_root_id,
            ar.creator_agent_id,
            a.handle as creator_agent_handle,
            ar.kind,
            ar.title,
            ar.summary,
            ar.content,
            ar.metadata,
            ar.created_at,
            ar.updated_at
        from artifacts ar
        left join agents a on a.id = ar.creator_agent_id
        order by ar.created_at asc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    Ok(rows.iter().map(artifact_from_row).collect())
}

async fn load_artifact(pool: &PgPool, artifact_id: Uuid) -> CommandResult<Artifact> {
    let row = sqlx::query(
        r#"
        select
            ar.id,
            ar.message_id,
            ar.channel_id,
            ar.thread_root_id,
            ar.creator_agent_id,
            a.handle as creator_agent_handle,
            ar.kind,
            ar.title,
            ar.summary,
            ar.content,
            ar.metadata,
            ar.created_at,
            ar.updated_at
        from artifacts ar
        left join agents a on a.id = ar.creator_agent_id
        where ar.id = $1
        "#,
    )
    .bind(artifact_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    Ok(artifact_from_row(&row))
}

async fn attach_message_artifacts(pool: &PgPool, messages: &mut [Message]) -> CommandResult<()> {
    if messages.is_empty() {
        return Ok(());
    }
    let ids: Vec<Uuid> = messages.iter().map(|message| message.id).collect();
    let rows = sqlx::query(
        r#"
        select
            ar.id,
            ar.message_id,
            ar.channel_id,
            ar.thread_root_id,
            ar.creator_agent_id,
            a.handle as creator_agent_handle,
            ar.kind,
            ar.title,
            ar.summary,
            ar.content,
            ar.metadata,
            ar.created_at,
            ar.updated_at
        from artifacts ar
        left join agents a on a.id = ar.creator_agent_id
        where ar.message_id = any($1)
        order by ar.created_at asc
        "#,
    )
    .bind(&ids)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    let mut artifacts_by_message: HashMap<Uuid, Vec<Artifact>> = HashMap::new();
    for row in rows {
        let artifact = artifact_from_row(&row);
        artifacts_by_message
            .entry(artifact.message_id)
            .or_default()
            .push(artifact);
    }
    for message in messages {
        message.artifacts = artifacts_by_message.remove(&message.id).unwrap_or_default();
    }
    Ok(())
}

async fn load_tasks(pool: &PgPool) -> CommandResult<Vec<Task>> {
    let rows = sqlx::query(
        r#"
        select
            t.id,
            t.number,
            t.message_id,
            t.channel_id,
            t.title,
            t.status,
            c.name as channel_name,
            t.assignee_agent_id as assignee_id,
            a.display_name as assignee_name,
            t.created_at,
            t.updated_at
        from tasks t
        join channels c on c.id = t.channel_id
        left join agents a on a.id = t.assignee_agent_id
        order by t.number desc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| Task {
            id: row.get("id"),
            number: row.get("number"),
            message_id: row.get("message_id"),
            channel_id: row.get("channel_id"),
            title: row.get("title"),
            status: row.get("status"),
            channel_name: row.get("channel_name"),
            assignee_id: row.get("assignee_id"),
            assignee_name: row.get("assignee_name"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

async fn load_reminders(pool: &PgPool) -> CommandResult<Vec<Reminder>> {
    let rows = sqlx::query(
        r#"
        select
            r.id,
            r.channel_id,
            c.name as channel_name,
            r.creator_agent_id,
            a.handle as creator_agent_handle,
            r.thread_root_id,
            r.message_id,
            r.title,
            r.note,
            r.status,
            r.recurrence,
            r.due_at,
            r.fired_at,
            r.completed_at,
            r.created_at,
            r.updated_at
        from reminders r
        left join channels c on c.id = r.channel_id
        left join agents a on a.id = r.creator_agent_id
        where r.status in ('scheduled', 'fired')
        order by
            case r.status when 'fired' then 0 else 1 end,
            r.due_at asc
        limit 100
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| Reminder {
            id: row.get("id"),
            channel_id: row.get("channel_id"),
            channel_name: row.get("channel_name"),
            creator_agent_id: row.get("creator_agent_id"),
            creator_agent_handle: row.get("creator_agent_handle"),
            thread_root_id: row.get("thread_root_id"),
            message_id: row.get("message_id"),
            title: row.get("title"),
            note: row.get("note"),
            status: row.get("status"),
            recurrence: row.get("recurrence"),
            due_at: row.get("due_at"),
            fired_at: row.get("fired_at"),
            completed_at: row.get("completed_at"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

async fn load_agent_schedules(pool: &PgPool) -> CommandResult<Vec<AgentSchedule>> {
    let rows = sqlx::query(
        r#"
        select
            s.id,
            s.agent_id,
            a.handle as agent_handle,
            s.channel_id,
            c.name as channel_name,
            c.kind as channel_kind,
            s.thread_root_id,
            s.title,
            s.prompt,
            s.cadence,
            s.status,
            s.next_run_at,
            s.last_run_at,
            s.last_work_item_id,
            s.created_at,
            s.updated_at
        from agent_schedules s
        join agents a on a.id = s.agent_id
        join channels c on c.id = s.channel_id
        where s.status in ('active', 'paused')
        order by
            case s.status when 'active' then 0 else 1 end,
            s.next_run_at asc
        limit 100
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| AgentSchedule {
            id: row.get("id"),
            agent_id: row.get("agent_id"),
            agent_handle: row.get("agent_handle"),
            channel_id: row.get("channel_id"),
            channel_name: row.get("channel_name"),
            channel_kind: row.get("channel_kind"),
            thread_root_id: row.get("thread_root_id"),
            title: row.get("title"),
            prompt: row.get("prompt"),
            cadence: row.get("cadence"),
            status: row.get("status"),
            next_run_at: row.get("next_run_at"),
            last_run_at: row.get("last_run_at"),
            last_work_item_id: row.get("last_work_item_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
        })
        .collect())
}

async fn load_agent_runs(pool: &PgPool) -> CommandResult<Vec<AgentRun>> {
    let rows = sqlx::query(
        r#"
        select
            r.id,
            r.agent_id,
            a.handle as agent_handle,
            r.work_item_id,
            r.command,
            r.working_directory,
            r.status,
            r.pid,
            r.exit_code,
            r.log,
            r.input_tokens,
            r.output_tokens,
            r.cost_micros,
            r.started_at,
            r.stopped_at
        from agent_runs r
        join agents a on a.id = r.agent_id
        order by r.started_at desc
        limit 30
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| AgentRun {
            id: row.get("id"),
            agent_id: row.get("agent_id"),
            agent_handle: row.get("agent_handle"),
            work_item_id: row.get("work_item_id"),
            command: row.get("command"),
            working_directory: row.get("working_directory"),
            status: row.get("status"),
            pid: row.get("pid"),
            exit_code: row.get("exit_code"),
            log: row.get("log"),
            input_tokens: row.get("input_tokens"),
            output_tokens: row.get("output_tokens"),
            cost_micros: row.get("cost_micros"),
            started_at: row.get("started_at"),
            stopped_at: row.get("stopped_at"),
        })
        .collect())
}

async fn load_agent_run_patch(pool: &PgPool, run_id: Uuid) -> CommandResult<AgentRunPatch> {
    let row = sqlx::query(
        r#"
        select
            r.id,
            r.agent_id,
            a.handle as agent_handle,
            r.work_item_id,
            r.command,
            r.working_directory,
            r.status,
            r.pid,
            r.exit_code,
            r.input_tokens,
            r.output_tokens,
            r.cost_micros,
            r.started_at,
            r.stopped_at
        from agent_runs r
        join agents a on a.id = r.agent_id
        where r.id = $1
        "#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    Ok(AgentRunPatch {
        id: row.get("id"),
        agent_id: row.get("agent_id"),
        agent_handle: row.get("agent_handle"),
        work_item_id: row.get("work_item_id"),
        command: row.get("command"),
        working_directory: row.get("working_directory"),
        status: row.get("status"),
        pid: row.get("pid"),
        exit_code: row.get("exit_code"),
        input_tokens: row.get("input_tokens"),
        output_tokens: row.get("output_tokens"),
        cost_micros: row.get("cost_micros"),
        started_at: row.get("started_at"),
        stopped_at: row.get("stopped_at"),
    })
}

async fn load_agent_work_items(pool: &PgPool) -> CommandResult<Vec<AgentWorkItem>> {
    let rows = sqlx::query(
        r#"
        select
            w.id,
            w.agent_id,
            a.handle as agent_handle,
            w.channel_id,
            c.name as channel_name,
            w.thread_root_id,
            w.source_message_id,
            w.task_id,
            t.number as task_number,
            w.source_kind,
            w.title,
            w.context,
            w.status,
            w.run_id,
            w.created_at,
            w.updated_at,
            w.completed_at
        from agent_work_items w
        join agents a on a.id = w.agent_id
        left join channels c on c.id = w.channel_id
        left join tasks t on t.id = w.task_id
        order by w.created_at desc
        limit 80
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| AgentWorkItem {
            id: row.get("id"),
            agent_id: row.get("agent_id"),
            agent_handle: row.get("agent_handle"),
            channel_id: row.get("channel_id"),
            channel_name: row.get("channel_name"),
            thread_root_id: row.get("thread_root_id"),
            source_message_id: row.get("source_message_id"),
            task_id: row.get("task_id"),
            task_number: row.get("task_number"),
            source_kind: row.get("source_kind"),
            title: row.get("title"),
            context: row.get("context"),
            status: row.get("status"),
            run_id: row.get("run_id"),
            created_at: row.get("created_at"),
            updated_at: row.get("updated_at"),
            completed_at: row.get("completed_at"),
        })
        .collect())
}

async fn load_agent_work_item_patch(
    pool: &PgPool,
    work_item_id: Uuid,
) -> CommandResult<AgentWorkItemPatch> {
    let row = sqlx::query(
        r#"
        select
            w.id,
            w.agent_id,
            a.handle as agent_handle,
            w.channel_id,
            c.name as channel_name,
            w.thread_root_id,
            w.source_message_id,
            w.task_id,
            t.number as task_number,
            w.source_kind,
            w.title,
            w.status,
            w.run_id,
            w.created_at,
            w.updated_at,
            w.completed_at
        from agent_work_items w
        join agents a on a.id = w.agent_id
        left join channels c on c.id = w.channel_id
        left join tasks t on t.id = w.task_id
        where w.id = $1
        "#,
    )
    .bind(work_item_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    Ok(AgentWorkItemPatch {
        id: row.get("id"),
        agent_id: row.get("agent_id"),
        agent_handle: row.get("agent_handle"),
        channel_id: row.get("channel_id"),
        channel_name: row.get("channel_name"),
        thread_root_id: row.get("thread_root_id"),
        source_message_id: row.get("source_message_id"),
        task_id: row.get("task_id"),
        task_number: row.get("task_number"),
        source_kind: row.get("source_kind"),
        title: row.get("title"),
        status: row.get("status"),
        run_id: row.get("run_id"),
        created_at: row.get("created_at"),
        updated_at: row.get("updated_at"),
        completed_at: row.get("completed_at"),
    })
}

async fn load_agent_activities(pool: &PgPool) -> CommandResult<Vec<AgentActivity>> {
    let rows = sqlx::query(
        r#"
        select
            id,
            agent_id,
            agent_handle,
            run_id,
            kind,
            phase,
            status,
            title,
            summary,
            detail,
            metadata::text as metadata,
            created_at
        from agent_activities
        order by created_at desc
        limit 80
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| AgentActivity {
            id: row.get("id"),
            agent_id: row.get("agent_id"),
            agent_handle: row.get("agent_handle"),
            run_id: row.get("run_id"),
            kind: row.get("kind"),
            phase: row.get("phase"),
            status: row.get("status"),
            title: row.get("title"),
            summary: row.get("summary"),
            detail: row.get("detail"),
            metadata: parse_json_value(row.get("metadata")),
            created_at: row.get("created_at"),
        })
        .collect())
}

async fn load_agent_activity(pool: &PgPool, activity_id: Uuid) -> CommandResult<AgentActivity> {
    let row = sqlx::query(
        r#"
        select
            id,
            agent_id,
            agent_handle,
            run_id,
            kind,
            phase,
            status,
            title,
            summary,
            detail,
            metadata::text as metadata,
            created_at
        from agent_activities
        where id = $1
        "#,
    )
    .bind(activity_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    Ok(AgentActivity {
        id: row.get("id"),
        agent_id: row.get("agent_id"),
        agent_handle: row.get("agent_handle"),
        run_id: row.get("run_id"),
        kind: row.get("kind"),
        phase: row.get("phase"),
        status: row.get("status"),
        title: row.get("title"),
        summary: row.get("summary"),
        detail: row.get("detail"),
        metadata: parse_json_value(row.get("metadata")),
        created_at: row.get("created_at"),
    })
}

async fn load_supervisor_status(pool: &PgPool) -> CommandResult<SupervisorStatus> {
    let row = sqlx::query(
        r#"
        select pid, status, updated_at
        from supervisor_state
        where id = 1
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;

    let Some(row) = row else {
        return Ok(SupervisorStatus {
            pid: None,
            status: "offline".to_owned(),
            updated_at: None,
        });
    };

    let updated_at: DateTime<Utc> = row.get("updated_at");
    let status = if Utc::now().signed_duration_since(updated_at).num_seconds() > 10 {
        "stale".to_owned()
    } else {
        row.get("status")
    };

    Ok(SupervisorStatus {
        pid: row.get("pid"),
        status,
        updated_at: Some(updated_at),
    })
}

fn load_launch_agent_status() -> CommandResult<LaunchAgentStatus> {
    let plist_path = launch_agent_plist_path()?;
    let installed = plist_path.exists();
    let loaded = launch_agent_domain()
        .map(|domain| {
            StdCommand::new("launchctl")
                .arg("print")
                .arg(launch_agent_service_target(&domain))
                .status()
                .map(|status| status.success())
                .unwrap_or(false)
        })
        .unwrap_or(false);

    Ok(LaunchAgentStatus {
        label: LAUNCH_AGENT_LABEL.to_owned(),
        plist_path: plist_path.to_string_lossy().to_string(),
        installed,
        loaded,
    })
}

fn parse_json_value(raw: String) -> Value {
    serde_json::from_str(&raw).unwrap_or_else(|_| json!({}))
}

fn activity_phase(kind: &str) -> &'static str {
    match kind {
        "thinking" => "thinking",
        "command" => "command",
        "file_edit" => "file_edit",
        "tools" => "tools",
        "error" | "event_error" | "run_error" => "error",
        "run" | "usage" => "runtime",
        "dispatch" | "mention" | "dm" | "task" | "schedule" | "channel" | "membership" => "work",
        "profile" | "memory" => "profile",
        _ => "acting",
    }
}

fn normalize_agent_activity_kind(kind: Option<&str>) -> &'static str {
    match kind.map(str::trim).filter(|kind| !kind.is_empty()) {
        Some("thinking") => "thinking",
        Some("command") | Some("running_command") => "command",
        Some("file_edit") | Some("editing_file") => "file_edit",
        Some("tools") | Some("tool") => "tools",
        Some("error") => "error",
        Some("task") => "task",
        Some("message") => "message",
        Some("dispatch") => "dispatch",
        Some("reminder") => "schedule",
        Some("schedule") => "schedule",
        Some("usage") => "usage",
        Some("memory") => "memory",
        Some("channel") => "channel",
        Some("membership") => "membership",
        _ => "acting",
    }
}

fn activity_status(kind: &str, title: &str) -> &'static str {
    let lowered = format!("{} {}", kind, title).to_lowercase();
    if matches!(kind, "error" | "event_error" | "run_error")
        || lowered.contains("failed")
        || lowered.contains("error")
        || lowered.contains("rejected")
    {
        "error"
    } else if lowered.contains("cancel") || lowered.contains("stop") || lowered.contains("stopping")
    {
        "warning"
    } else if lowered.contains("completed")
        || lowered.contains("complete")
        || lowered.contains("done")
        || lowered.contains("exited")
        || lowered.contains("finished")
        || lowered.contains("ready")
        || lowered.contains("accepted")
    {
        "success"
    } else if lowered.contains("running")
        || lowered.contains("started")
        || lowered.contains("queued")
        || lowered.contains("dispatched")
        || lowered.contains("responding")
        || lowered.contains("thinking")
        || lowered.contains("editing")
        || lowered.contains("using")
    {
        "active"
    } else {
        "info"
    }
}

fn work_status_title(status: &str) -> &'static str {
    match status {
        "running" => "Request started",
        "done" => "Request completed",
        "silent" => "No visible reply needed",
        "cancelled" => "Request cancelled",
        "failed" => "Request failed",
        "queued" => "Request queued",
        _ => "Request updated",
    }
}

fn parse_activity_metadata(detail: &str) -> Value {
    let detail = detail.trim();
    if detail.is_empty() {
        return json!({});
    }
    if let Ok(value) = serde_json::from_str::<Value>(detail) {
        if value.is_object() {
            return value;
        }
    }

    let mut metadata = serde_json::Map::new();
    for segment in detail.split([',', '\n']) {
        let Some((key, value)) = segment.split_once('=') else {
            continue;
        };
        let key = key.trim();
        let value = value.trim();
        if key.is_empty() || value.is_empty() {
            continue;
        }
        metadata.insert(key.to_owned(), json!(value));
        if key.ends_with("duration") || key == "duration" {
            if let Some(ms) = value
                .split_whitespace()
                .next()
                .and_then(|value| value.parse::<u64>().ok())
            {
                metadata.insert("duration_ms".to_owned(), json!(ms));
            }
        }
    }

    if metadata.is_empty() {
        if Uuid::parse_str(detail).is_ok() {
            metadata.insert("reference_id".to_owned(), json!(detail));
        } else {
            metadata.insert("detail".to_owned(), json!(detail));
        }
    }

    Value::Object(metadata)
}

fn value_i64_at(value: &Value, path: &str) -> Option<i64> {
    value.pointer(path).and_then(|value| {
        value
            .as_i64()
            .or_else(|| value.as_u64().and_then(|value| i64::try_from(value).ok()))
            .or_else(|| value.as_f64().map(|value| value.round() as i64))
    })
}

fn usage_from_runtime_event(value: &Value) -> Option<(i64, i64)> {
    let input_tokens = [
        "/params/tokenUsage/last/inputTokens",
        "/params/tokenUsage/last/input_tokens",
        "/params/tokenUsage/last/promptTokens",
        "/params/tokenUsage/last/prompt_tokens",
        "/params/usage/input_tokens",
        "/params/usage/inputTokens",
        "/params/usage/input",
        "/params/usage/prompt_tokens",
        "/params/usage/promptTokens",
        "/usage/input_tokens",
        "/usage/inputTokens",
        "/usage/prompt_tokens",
        "/message/usage/input_tokens",
        "/message/usage/prompt_tokens",
        "/params/tokenUsage/total/inputTokens",
        "/params/tokenUsage/total/input_tokens",
    ]
    .iter()
    .find_map(|path| value_i64_at(value, path))
    .unwrap_or_default();
    let output_tokens = [
        "/params/tokenUsage/last/outputTokens",
        "/params/tokenUsage/last/output_tokens",
        "/params/tokenUsage/last/completionTokens",
        "/params/tokenUsage/last/completion_tokens",
        "/params/usage/output_tokens",
        "/params/usage/outputTokens",
        "/params/usage/output",
        "/params/usage/completion_tokens",
        "/params/usage/completionTokens",
        "/usage/output_tokens",
        "/usage/outputTokens",
        "/usage/completion_tokens",
        "/message/usage/output_tokens",
        "/message/usage/completion_tokens",
        "/params/tokenUsage/total/outputTokens",
        "/params/tokenUsage/total/output_tokens",
    ]
    .iter()
    .find_map(|path| value_i64_at(value, path))
    .unwrap_or_default();

    (input_tokens > 0 || output_tokens > 0).then_some((input_tokens.max(0), output_tokens.max(0)))
}

fn usage_from_run_log(log: &str) -> Option<(i64, i64)> {
    log.lines()
        .filter_map(|line| {
            let json_start = line.find('{')?;
            let value = serde_json::from_str::<Value>(&line[json_start..]).ok()?;
            usage_from_runtime_event(&value)
        })
        .last()
}

fn model_cost_micros(runtime: &str, model: &str, input_tokens: i64, output_tokens: i64) -> i64 {
    let model = model.to_lowercase();
    let runtime = runtime.to_lowercase();
    let (input_per_million, output_per_million) = if runtime == "claude" {
        if model.contains("opus") {
            (15_000_000_i64, 75_000_000_i64)
        } else if model.contains("haiku") {
            (250_000_i64, 1_250_000_i64)
        } else {
            (3_000_000_i64, 15_000_000_i64)
        }
    } else if model.contains("mini") {
        (150_000_i64, 600_000_i64)
    } else if model.contains("codex") {
        (1_500_000_i64, 6_000_000_i64)
    } else {
        (1_000_000_i64, 5_000_000_i64)
    };
    ((input_tokens.max(0) * input_per_million) + (output_tokens.max(0) * output_per_million))
        / 1_000_000
}

async fn record_run_usage(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    input_tokens: i64,
    output_tokens: i64,
    cost_micros: Option<i64>,
) -> CommandResult<()> {
    let row = sqlx::query("select runtime, model from agents where id = $1")
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let runtime: String = row.get("runtime");
    let model: String = row.get("model");
    let estimated_cost = cost_micros
        .unwrap_or_else(|| model_cost_micros(&runtime, &model, input_tokens, output_tokens))
        .max(0);
    sqlx::query(
        r#"
        update agent_runs
        set input_tokens = greatest(input_tokens, $2),
            output_tokens = greatest(output_tokens, $3),
            cost_micros = greatest(cost_micros, $4)
        where id = $1
        "#,
    )
    .bind(run_id)
    .bind(input_tokens.max(0))
    .bind(output_tokens.max(0))
    .bind(estimated_cost)
    .execute(pool)
    .await
    .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_usage").await;
    Ok(())
}

async fn backfill_agent_run_usage_from_logs(pool: &PgPool) -> sqlx::Result<()> {
    let rows = sqlx::query(
        r#"
        select id, agent_id, log
        from agent_runs
        where input_tokens = 0
          and output_tokens = 0
          and log like '%tokenUsage%'
        order by started_at desc
        limit 200
        "#,
    )
    .fetch_all(pool)
    .await?;

    for row in rows {
        let log: String = row.get("log");
        let Some((input_tokens, output_tokens)) = usage_from_run_log(&log) else {
            continue;
        };
        let run_id: Uuid = row.get("id");
        let agent_id: Uuid = row.get("agent_id");
        let agent = sqlx::query("select runtime, model from agents where id = $1")
            .bind(agent_id)
            .fetch_one(pool)
            .await?;
        let runtime: String = agent.get("runtime");
        let model: String = agent.get("model");
        let cost_micros = model_cost_micros(&runtime, &model, input_tokens, output_tokens);
        sqlx::query(
            r#"
            update agent_runs
            set input_tokens = $2,
                output_tokens = $3,
                cost_micros = $4
            where id = $1
            "#,
        )
        .bind(run_id)
        .bind(input_tokens.max(0))
        .bind(output_tokens.max(0))
        .bind(cost_micros.max(0))
        .execute(pool)
        .await?;
    }

    Ok(())
}

async fn agent_budget_exhausted(pool: &PgPool, agent_id: Uuid) -> CommandResult<Option<String>> {
    let daily_budget_micros: i64 =
        sqlx::query_scalar("select daily_budget_micros from agents where id = $1")
            .bind(agent_id)
            .fetch_one(pool)
            .await
            .map_err(to_string)?;
    if daily_budget_micros <= 0 {
        return Ok(None);
    }
    let spent: i64 = sqlx::query_scalar(
        r#"
        select coalesce(sum(cost_micros), 0)::bigint
        from agent_runs
        where agent_id = $1
          and started_at >= date_trunc('day', now())
        "#,
    )
    .bind(agent_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    if spent >= daily_budget_micros {
        Ok(Some(format!(
            "daily budget reached: spent ${:.4} / ${:.4}",
            spent as f64 / 1_000_000.0,
            daily_budget_micros as f64 / 1_000_000.0
        )))
    } else {
        Ok(None)
    }
}

fn memory_path_for_workspace(working_directory: &str) -> CommandResult<PathBuf> {
    let working_directory = working_directory.trim();
    if working_directory.is_empty() {
        return Err("agent working_directory is not configured".to_owned());
    }
    let workspace = PathBuf::from(working_directory);
    fs::create_dir_all(&workspace).map_err(to_string)?;
    Ok(workspace.join("MEMORY.md"))
}

async fn agent_memory_path(pool: &PgPool, agent_id: Uuid) -> CommandResult<PathBuf> {
    let row = sqlx::query("select handle, working_directory from agents where id = $1")
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let handle: String = row.get("handle");
    let working_directory: String = row.get("working_directory");
    ensure_agent_workspace(working_directory.trim(), &handle)?;
    memory_path_for_workspace(&working_directory)
}

async fn append_agent_memory(pool: &PgPool, agent_id: Uuid, body: &str) -> CommandResult<()> {
    let body = body.trim();
    if body.is_empty() {
        return Err("memory_append body is empty".to_owned());
    }
    let path = agent_memory_path(pool, agent_id).await?;
    let entry = format!(
        "\n\n## Memory update {}\n{}\n",
        Utc::now().to_rfc3339(),
        body
    );
    fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .and_then(|mut file| std::io::Write::write_all(&mut file, entry.as_bytes()))
        .map_err(to_string)
}

async fn compact_agent_memory(pool: &PgPool, agent_id: Uuid, body: &str) -> CommandResult<()> {
    let body = body.trim();
    if body.is_empty() {
        return Err("memory_compact body is empty".to_owned());
    }
    let path = agent_memory_path(pool, agent_id).await?;
    if path.exists() {
        let backup = path.with_extension(format!("md.bak-{}", Utc::now().format("%Y%m%d%H%M%S")));
        let _ = fs::copy(&path, backup);
    }
    fs::write(path, format!("{body}\n")).map_err(to_string)
}

async fn create_channel_in_pool(
    pool: &PgPool,
    name: &str,
    description: &str,
) -> CommandResult<Uuid> {
    let normalized = normalize_channel_name(name);
    if normalized.is_empty() {
        return Err("channel name is empty".to_owned());
    }
    sqlx::query_scalar(
        r#"
        insert into channels (name, description, kind)
        values ($1, $2, 'channel')
        on conflict (name) do update
            set description = case
                when excluded.description <> '' then excluded.description
                else channels.description
            end
        returning id
        "#,
    )
    .bind(normalized)
    .bind(description.trim())
    .fetch_one(pool)
    .await
    .map_err(to_string)
}

async fn add_agent_to_channel(
    pool: &PgPool,
    channel_id: Uuid,
    agent_id: Uuid,
) -> CommandResult<()> {
    let kind: Option<String> = sqlx::query_scalar("select kind from channels where id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?;
    if kind.as_deref() == Some("dm") {
        return Err("direct message membership is fixed".to_owned());
    }
    sqlx::query(
        r#"
        insert into channel_members (channel_id, agent_id)
        values ($1, $2)
        on conflict (channel_id, agent_id) do nothing
        "#,
    )
    .bind(channel_id)
    .bind(agent_id)
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

async fn record_agent_activity(
    pool: &PgPool,
    agent_id: Option<Uuid>,
    run_id: Option<Uuid>,
    kind: &str,
    title: impl AsRef<str>,
    detail: impl AsRef<str>,
) -> CommandResult<()> {
    let agent_handle = match agent_id {
        Some(agent_id) => sqlx::query_scalar("select handle from agents where id = $1")
            .bind(agent_id)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?
            .unwrap_or_else(|| "unknown".to_owned()),
        None => String::new(),
    };
    let title = title.as_ref();
    let detail = detail.as_ref();
    let phase = activity_phase(kind);
    let status = activity_status(kind, title);
    let summary = title;
    let metadata = parse_activity_metadata(detail);

    let activity_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_activities (
            agent_id,
            agent_handle,
            run_id,
            kind,
            phase,
            status,
            title,
            summary,
            detail,
            metadata
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10::jsonb)
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(agent_handle)
    .bind(run_id)
    .bind(kind)
    .bind(phase)
    .bind(status)
    .bind(title)
    .bind(summary)
    .bind(detail)
    .bind(metadata.to_string())
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    if let Ok(activity) = load_agent_activity(pool, activity_id).await {
        let _ = notify_ui_activity_upsert(pool, &activity, "activity").await;
    } else {
        let _ = notify_ui_refresh(pool, "activity").await;
    }

    Ok(())
}

async fn record_agent_activity_throttled(
    pool: &PgPool,
    agent_id: Option<Uuid>,
    run_id: Option<Uuid>,
    kind: &str,
    title: impl AsRef<str>,
    detail: impl AsRef<str>,
) -> CommandResult<()> {
    let title = title.as_ref();
    let detail = detail.as_ref();
    let recently_recorded: bool = sqlx::query_scalar(
        r#"
        select exists (
            select 1
            from agent_activities
            where agent_id is not distinct from $1
              and run_id is not distinct from $2
              and kind = $3
              and title = $4
              and detail = $5
              and created_at > now() - interval '1 second'
        )
        "#,
    )
    .bind(agent_id)
    .bind(run_id)
    .bind(kind)
    .bind(title)
    .bind(detail)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    if recently_recorded {
        return Ok(());
    }

    record_agent_activity(pool, agent_id, run_id, kind, title, detail).await
}

async fn append_run_log(pool: &PgPool, run_id: Uuid, line: String) -> CommandResult<()> {
    sqlx::query("update agent_runs set log = right(log || $2, 20000) where id = $1")
        .bind(run_id)
        .bind(line)
        .execute(pool)
        .await
        .map_err(to_string)?;

    Ok(())
}

fn truncate_activity_detail(value: &str) -> String {
    let trimmed = value.trim();
    let mut chars = trimmed.chars();
    let mut out: String = chars.by_ref().take(600).collect();
    if chars.next().is_some() {
        out.push_str("...");
    }
    out
}

fn classify_agent_output_activity(
    label: &str,
    line: &str,
) -> Option<(&'static str, &'static str, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || extract_agent_event_json(trimmed).is_some() {
        return None;
    }

    let lower = trimmed.to_lowercase();
    let is_error = label == "stderr"
        && [
            "error",
            "failed",
            "panic",
            "exception",
            "traceback",
            "permission denied",
            "not found",
        ]
        .iter()
        .any(|needle| lower.contains(needle));
    if is_error {
        return Some(("error", "Error output", truncate_activity_detail(trimmed)));
    }

    let is_tool = [
        "tool",
        "exec_command",
        "apply_patch",
        "running command",
        "cargo ",
        "npm ",
        "git ",
        "psql ",
        "slock ",
        "rg ",
        "sed ",
    ]
    .iter()
    .any(|needle| lower.contains(needle))
        || trimmed.starts_with("$ ");
    if is_tool {
        return Some(("tools", "Using tools", truncate_activity_detail(trimmed)));
    }

    let is_action = [
        "message sent",
        "created",
        "updated",
        "deleted",
        "fixed",
        "committed",
        "commit ",
        "done",
        "completed",
        "finished",
    ]
    .iter()
    .any(|needle| lower.contains(needle));
    if is_action {
        return Some(("acting", "Acting", truncate_activity_detail(trimmed)));
    }

    Some(("thinking", "Thinking", truncate_activity_detail(trimmed)))
}

async fn pipe_run_output<R>(
    pool: PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    stream: R,
    label: &'static str,
    parse_agent_events: bool,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let _ = append_run_log(&pool, run_id, format!("[{label}] {line}\n")).await;
                if let Some((kind, title, detail)) = classify_agent_output_activity(label, &line) {
                    let _ = record_agent_activity_throttled(
                        &pool,
                        Some(agent_id),
                        Some(run_id),
                        kind,
                        title,
                        detail,
                    )
                    .await;
                }
                if parse_agent_events {
                    if let Some(reason) = silent_reply_reason(&line) {
                        let _ = mark_run_work_item_silent(&pool, agent_id, run_id, &reason).await;
                        continue;
                    }
                    match ingest_agent_event_line(&pool, agent_id, run_id, &line).await {
                        Ok(Some(note)) => {
                            let _ =
                                append_run_log(&pool, run_id, format!("[event] {note}\n")).await;
                            let _ = record_agent_activity(
                                &pool,
                                Some(agent_id),
                                Some(run_id),
                                "event",
                                "Stdout event accepted",
                                note,
                            )
                            .await;
                        }
                        Ok(None) => {}
                        Err(err) => {
                            let _ =
                                append_run_log(&pool, run_id, format!("[event] rejected: {err}\n"))
                                    .await;
                            let _ = record_agent_activity(
                                &pool,
                                Some(agent_id),
                                Some(run_id),
                                "event_error",
                                "Stdout event rejected",
                                err.to_string(),
                            )
                            .await;
                        }
                    }
                }
            }
            Ok(None) => break,
            Err(err) => {
                let _ =
                    append_run_log(&pool, run_id, format!("[{label}] read error: {err}\n")).await;
                break;
            }
        }
    }
}

async fn ingest_agent_event_line(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    line: &str,
) -> CommandResult<Option<String>> {
    let Some(json) = extract_agent_event_json(line) else {
        return Ok(None);
    };
    if !claim_agent_event(pool, run_id, json).await? {
        return Ok(None);
    }
    let event: AgentEvent = serde_json::from_str(json).map_err(to_string)?;
    handle_agent_event(pool, agent_id, run_id, event)
        .await
        .map(Some)
}

async fn claim_agent_event(pool: &PgPool, run_id: Uuid, json: &str) -> CommandResult<bool> {
    let inserted: Option<bool> = sqlx::query_scalar(
        r#"
        insert into agent_event_receipts (run_id, event_json)
        values ($1, $2)
        on conflict (run_id, event_json) do nothing
        returning true
        "#,
    )
    .bind(run_id)
    .bind(json)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;

    Ok(inserted.unwrap_or(false))
}

async fn replay_agent_events_from_run_log_if_needed(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
) -> CommandResult<usize> {
    let accepted_events: i64 = sqlx::query_scalar(
        r#"
        select count(*)
        from agent_activities
        where run_id = $1
          and kind = 'event'
          and title = 'Stdout event accepted'
        "#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    if accepted_events > 0 {
        return Ok(0);
    }

    let Some(log): Option<String> = sqlx::query_scalar("select log from agent_runs where id = $1")
        .bind(run_id)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?
    else {
        return Ok(0);
    };

    let mut replayed = 0;
    let mut seen = HashSet::new();
    for line in log.lines() {
        let Some(json) = extract_agent_event_json(line) else {
            continue;
        };
        if !seen.insert(json.to_owned()) {
            continue;
        }
        let result = match serde_json::from_str::<AgentEvent>(json).map_err(to_string) {
            Ok(event) => {
                if claim_agent_event(pool, run_id, json).await? {
                    handle_agent_event(pool, agent_id, run_id, event).await
                } else {
                    continue;
                }
            }
            Err(err) => Err(err),
        };
        match result {
            Ok(note) => {
                replayed += 1;
                append_run_log(pool, run_id, format!("[event-replay] {note}\n")).await?;
                record_agent_activity(
                    pool,
                    Some(agent_id),
                    Some(run_id),
                    "event",
                    "Run log event replayed",
                    note,
                )
                .await?;
            }
            Err(err) => {
                append_run_log(pool, run_id, format!("[event-replay] rejected: {err}\n")).await?;
                record_agent_activity(
                    pool,
                    Some(agent_id),
                    Some(run_id),
                    "event_error",
                    "Run log event rejected",
                    err,
                )
                .await?;
            }
        }
    }

    Ok(replayed)
}

fn extract_agent_event_json(line: &str) -> Option<&str> {
    let mut trimmed = line.trim();
    for prefix in ["[stdout] ", "[stderr] "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            trimmed = rest.trim_start();
            break;
        }
    }
    trimmed.strip_prefix(AGENT_EVENT_PREFIX).map(str::trim)
}

async fn handle_agent_event(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    event: AgentEvent,
) -> CommandResult<String> {
    match event {
        AgentEvent::Message {
            channel,
            channel_id,
            thread_root_id,
            body,
            as_task,
        } => {
            let channel_id = resolve_event_channel(pool, channel_id, channel.as_deref()).await?;
            let msg_id = insert_agent_message(
                pool,
                agent_id,
                channel_id,
                thread_root_id,
                body.trim(),
                as_task.unwrap_or(false),
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                if as_task.unwrap_or(false) {
                    "task"
                } else {
                    "message"
                },
                if as_task.unwrap_or(false) {
                    "Task created from stdout event"
                } else {
                    "Message posted from stdout event"
                },
                format!("message_id={msg_id}"),
            )
            .await?;
            Ok(format!("message accepted {msg_id}"))
        }
        AgentEvent::Activity {
            kind,
            title,
            detail,
        } => {
            let title = title.trim();
            if title.is_empty() {
                return Err("activity title is required".to_owned());
            }
            let kind = normalize_agent_activity_kind(kind.as_deref());
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                kind,
                title,
                detail.unwrap_or_default(),
            )
            .await?;
            Ok("activity accepted".to_owned())
        }
        AgentEvent::TaskCreate {
            channel,
            channel_id,
            title,
            body,
            thread_body,
            assign_self,
            status,
        } => {
            let channel_id = resolve_event_channel(pool, channel_id, channel.as_deref()).await?;
            let (task_number, root_message_id, thread_reply_id) = create_agent_task_thread(
                pool,
                agent_id,
                channel_id,
                &title,
                body.as_deref(),
                thread_body.as_deref(),
                assign_self.unwrap_or(true),
                status.as_deref(),
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "task",
                format!("Task #{task_number} created"),
                json!({
                    "task_number": task_number,
                    "message_id": root_message_id,
                    "thread_reply_id": thread_reply_id,
                })
                .to_string(),
            )
            .await?;
            Ok(format!("task #{task_number} created {root_message_id}"))
        }
        AgentEvent::TaskStatus {
            task_number,
            status,
        } => {
            let status = status.trim();
            if !matches!(status, "todo" | "in_progress" | "in_review" | "done") {
                return Err(format!("unsupported task status: {status}"));
            }
            let row = sqlx::query(
                r#"
                update tasks
                set status = $2, updated_at = now()
                where number = $1
                returning channel_id, title
                "#,
            )
            .bind(task_number)
            .bind(status)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?
            .ok_or_else(|| format!("task #{task_number} does not exist"))?;
            let channel_id: Uuid = row.get("channel_id");
            let title: String = row.get("title");
            insert_system_message(
                pool,
                channel_id,
                None,
                format!(
                    "Task #{task_number} moved to {}: {title}",
                    status_label(status)
                ),
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "task",
                format!("Task #{task_number} status changed"),
                format!("status={status}"),
            )
            .await?;
            Ok(format!("task #{task_number} status set to {status}"))
        }
        AgentEvent::TaskClaim {
            task_number,
            assignee_handle,
        } => {
            let assignee = match assignee_handle.as_deref().map(str::trim) {
                Some("") | Some("null") | Some("unassigned") => None,
                Some(handle) => Some(resolve_agent_by_handle(pool, handle).await?),
                None => Some(agent_id),
            };
            let row = sqlx::query(
                r#"
                with updated as (
                    update tasks
                    set assignee_agent_id = $2,
                        status = case when $2 is null then status else 'in_progress' end,
                        updated_at = now()
                    where number = $1
                    returning channel_id, title, assignee_agent_id
                )
                select
                    u.channel_id,
                    u.title,
                    a.handle as assignee_handle
                from updated u
                left join agents a on a.id = u.assignee_agent_id
                "#,
            )
            .bind(task_number)
            .bind(assignee)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?
            .ok_or_else(|| format!("task #{task_number} does not exist"))?;
            let channel_id: Uuid = row.get("channel_id");
            let title: String = row.get("title");
            let body = if let Some(handle) = row.get::<Option<String>, _>("assignee_handle") {
                format!("@{handle} claimed task #{task_number}: {title}")
            } else {
                format!("Task #{task_number} was unassigned: {title}")
            };
            insert_system_message(pool, channel_id, None, body).await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "task",
                format!("Task #{task_number} assignee changed"),
                assignee_handle
                    .as_deref()
                    .unwrap_or("claimed by current agent"),
            )
            .await?;
            Ok(format!("task #{task_number} assignee updated"))
        }
        AgentEvent::Silent { reason } => {
            let reason =
                reason.unwrap_or_else(|| "Agent judged no visible reply was needed.".to_owned());
            mark_run_work_item_silent(pool, agent_id, run_id, &reason).await?;
            Ok("silent reply accepted".to_owned())
        }
        AgentEvent::ReminderCreate {
            channel,
            channel_id,
            thread_root_id,
            message_id,
            title,
            note,
            due_at,
            recurrence,
        } => {
            let due_at = due_at.ok_or_else(|| {
                "reminder_create requires a when or due_at ISO8601 timestamp".to_owned()
            })?;
            let due_at = parse_due_at(&due_at)?;
            let (default_channel_id, default_thread_root_id, default_message_id) =
                resolve_run_reminder_anchor(pool, agent_id, run_id).await?;
            let resolved_channel_id = if channel_id.is_some() || channel.is_some() {
                Some(resolve_event_channel(pool, channel_id, channel.as_deref()).await?)
            } else {
                default_channel_id
            };
            let reminder_id = create_reminder_in_pool(
                pool,
                Some(agent_id),
                resolved_channel_id,
                thread_root_id.or(default_thread_root_id),
                message_id.or(default_message_id),
                &title,
                note.as_deref().unwrap_or(""),
                due_at,
                recurrence.as_deref().unwrap_or("none"),
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "reminder",
                "Reminder scheduled",
                json!({
                    "reminder_id": reminder_id,
                    "title": title.trim(),
                    "due_at": due_at.to_rfc3339(),
                    "recurrence": recurrence.unwrap_or_else(|| "none".to_owned())
                })
                .to_string(),
            )
            .await?;
            Ok(format!("reminder created {reminder_id}"))
        }
        AgentEvent::ReminderCancel { reminder_id } => {
            cancel_reminder_in_pool(pool, reminder_id).await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "reminder",
                "Reminder cancelled",
                json!({ "reminder_id": reminder_id }).to_string(),
            )
            .await?;
            Ok(format!("reminder cancelled {reminder_id}"))
        }
        AgentEvent::Usage {
            input_tokens,
            output_tokens,
            total_tokens,
            cost_micros,
            cost_usd,
        } => {
            let input_tokens = input_tokens.unwrap_or_default().max(0);
            let mut output_tokens = output_tokens.unwrap_or_default().max(0);
            if output_tokens == 0 {
                if let Some(total_tokens) = total_tokens {
                    output_tokens = (total_tokens - input_tokens).max(0);
                }
            }
            let event_cost_micros = cost_micros
                .or_else(|| cost_usd.map(|value| (value.max(0.0) * 1_000_000.0).round() as i64));
            record_run_usage(
                pool,
                agent_id,
                run_id,
                input_tokens,
                output_tokens,
                event_cost_micros,
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "usage",
                "Usage recorded",
                json!({
                    "input_tokens": input_tokens,
                    "output_tokens": output_tokens,
                    "cost_micros": event_cost_micros
                })
                .to_string(),
            )
            .await?;
            Ok("usage accepted".to_owned())
        }
        AgentEvent::MemoryAppend { body } => {
            append_agent_memory(pool, agent_id, &body).await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "memory",
                "Memory updated",
                json!({ "operation": "append" }).to_string(),
            )
            .await?;
            Ok("memory appended".to_owned())
        }
        AgentEvent::MemoryCompact { body } => {
            compact_agent_memory(pool, agent_id, &body).await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "memory",
                "Memory compacted",
                json!({ "operation": "compact" }).to_string(),
            )
            .await?;
            Ok("memory compacted".to_owned())
        }
        AgentEvent::ChannelCreate {
            name,
            description,
            agent_handles,
        } => {
            let channel_id =
                create_channel_in_pool(pool, &name, description.as_deref().unwrap_or("")).await?;
            add_agent_to_channel(pool, channel_id, agent_id).await?;
            let mut invited = Vec::new();
            for handle in agent_handles.unwrap_or_default() {
                let invited_agent_id = resolve_agent_by_handle(pool, &handle).await?;
                add_agent_to_channel(pool, channel_id, invited_agent_id).await?;
                invited.push(handle.trim().trim_start_matches('@').to_owned());
            }
            insert_system_message(
                pool,
                channel_id,
                None,
                format!(
                    "@{} created #{}{}",
                    sqlx::query_scalar::<_, String>("select handle from agents where id = $1")
                        .bind(agent_id)
                        .fetch_one(pool)
                        .await
                        .map_err(to_string)?,
                    normalize_channel_name(&name),
                    if invited.is_empty() {
                        String::new()
                    } else {
                        format!(
                            " and invited {}",
                            invited
                                .iter()
                                .map(|h| format!("@{h}"))
                                .collect::<Vec<_>>()
                                .join(", ")
                        )
                    }
                ),
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "channel",
                "Channel created",
                json!({
                    "channel_id": channel_id,
                    "name": normalize_channel_name(&name),
                    "invited": invited
                })
                .to_string(),
            )
            .await?;
            Ok(format!("channel created {channel_id}"))
        }
        AgentEvent::ChannelInvite {
            channel,
            channel_id,
            agent_handles,
        } => {
            if agent_handles.is_empty() {
                return Err("channel_invite requires agent_handles".to_owned());
            }
            let channel_id = resolve_event_channel(pool, channel_id, channel.as_deref()).await?;
            let mut invited = Vec::new();
            for handle in agent_handles {
                let invited_agent_id = resolve_agent_by_handle(pool, &handle).await?;
                add_agent_to_channel(pool, channel_id, invited_agent_id).await?;
                invited.push(handle.trim().trim_start_matches('@').to_owned());
            }
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "membership",
                "Agents invited",
                json!({
                    "channel_id": channel_id,
                    "invited": invited
                })
                .to_string(),
            )
            .await?;
            let _ = notify_ui_refresh(pool, "channel_invite").await;
            Ok("agents invited".to_owned())
        }
        AgentEvent::ProfileUpdate {
            display_name,
            role,
            avatar,
            description,
        } => {
            let display_name = display_name
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let role = role
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let avatar = avatar
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            let description = description
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty());
            if display_name.is_none() && role.is_none() && avatar.is_none() && description.is_none()
            {
                return Err("profile_update requires at least one non-empty field".to_owned());
            }
            sqlx::query(
                r#"
                update agents
                set display_name = coalesce($2, display_name),
                    role = coalesce($3, role),
                    avatar = coalesce($4, avatar),
                    description = coalesce($5, description)
                where id = $1
                "#,
            )
            .bind(agent_id)
            .bind(display_name)
            .bind(role)
            .bind(avatar)
            .bind(description)
            .execute(pool)
            .await
            .map_err(to_string)?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "profile",
                "Profile updated",
                json!({
                    "display_name": display_name,
                    "role": role,
                    "avatar": avatar,
                    "description": description
                })
                .to_string(),
            )
            .await?;
            let _ = notify_ui_refresh(pool, "profile_update").await;
            Ok("profile updated".to_owned())
        }
        AgentEvent::ArtifactCreate {
            channel,
            channel_id,
            thread_root_id,
            kind,
            title,
            summary,
            content,
            metadata,
        } => {
            let channel_id = resolve_event_channel(pool, channel_id, channel.as_deref()).await?;
            let (artifact_id, message_id) = create_agent_artifact(
                pool,
                agent_id,
                channel_id,
                thread_root_id,
                &kind,
                &title,
                summary.as_deref(),
                &content,
                metadata,
            )
            .await?;
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "artifact",
                "Artifact created",
                json!({
                    "artifact_id": artifact_id,
                    "message_id": message_id,
                    "kind": kind,
                    "title": title
                })
                .to_string(),
            )
            .await?;
            Ok(format!("artifact created: {artifact_id}"))
        }
    }
}

async fn resolve_run_reminder_anchor(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
) -> CommandResult<(Option<Uuid>, Option<Uuid>, Option<Uuid>)> {
    let row = sqlx::query(
        r#"
        select w.channel_id, w.thread_root_id, w.source_message_id
        from agent_runs r
        left join agent_work_items w on w.id = r.work_item_id
        where r.id = $1 and r.agent_id = $2
        "#,
    )
    .bind(run_id)
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;
    Ok(row
        .map(|row| {
            (
                row.get("channel_id"),
                row.get("thread_root_id"),
                row.get("source_message_id"),
            )
        })
        .unwrap_or((None, None, None)))
}

async fn resolve_event_channel(
    pool: &PgPool,
    channel_id: Option<Uuid>,
    channel_name: Option<&str>,
) -> CommandResult<Uuid> {
    if let Some(channel_id) = channel_id {
        let exists: Option<Uuid> = sqlx::query_scalar("select id from channels where id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?;
        return exists.ok_or_else(|| format!("channel {channel_id} does not exist"));
    }

    let Some(name) = channel_name else {
        return Err("message event requires channel or channel_id".to_owned());
    };
    let normalized = normalize_channel_name(name);
    if normalized.is_empty() {
        return Err("message event channel is empty".to_owned());
    }
    sqlx::query_scalar("select id from channels where name = $1")
        .bind(&normalized)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?
        .ok_or_else(|| format!("channel #{normalized} does not exist"))
}

async fn resolve_agent_by_handle(pool: &PgPool, handle: &str) -> CommandResult<Uuid> {
    let normalized = handle.trim().trim_start_matches('@');
    if normalized.is_empty() {
        return Err("assignee handle is empty".to_owned());
    }
    sqlx::query_scalar("select id from agents where handle = $1")
        .bind(normalized)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?
        .ok_or_else(|| format!("agent @{normalized} does not exist"))
}

async fn insert_agent_message(
    pool: &PgPool,
    agent_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    body: &str,
    as_task: bool,
) -> CommandResult<Uuid> {
    if body.is_empty() {
        return Err("message event body is empty".to_owned());
    }
    if as_task && thread_root_id.is_some() {
        return Err("task message events must be root messages".to_owned());
    }
    if as_task {
        let channel_kind: Option<String> =
            sqlx::query_scalar("select kind from channels where id = $1")
                .bind(channel_id)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?;
        if channel_kind.as_deref() == Some("dm") {
            return Err("direct messages do not support tasks".to_owned());
        }
    }
    if let Some(thread_root_id) = thread_root_id {
        let root_channel: Option<Uuid> = sqlx::query_scalar(
            "select channel_id from messages where id = $1 and thread_root_id is null",
        )
        .bind(thread_root_id)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?;
        if root_channel != Some(channel_id) {
            return Err("thread_root_id does not belong to target channel".to_owned());
        }
    }

    let sender = sqlx::query("select display_name, role from agents where id = $1")
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let sender_name: String = sender.get("display_name");
    let sender_role: String = sender.get("role");

    let mut tx = pool.begin().await.map_err(to_string)?;
    let msg_id: Uuid = sqlx::query_scalar(
        r#"
        insert into messages (
            channel_id, thread_root_id, sender_agent_id, sender_name, sender_role, body, is_task
        )
        values ($1, $2, $3, $4, $5, $6, $7)
        returning id
        "#,
    )
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(agent_id)
    .bind(sender_name)
    .bind(sender_role)
    .bind(body)
    .bind(as_task)
    .fetch_one(&mut *tx)
    .await
    .map_err(to_string)?;

    if as_task {
        sqlx::query(
            r#"
            insert into tasks (message_id, channel_id, title, status, assignee_agent_id)
            values ($1, $2, $3, 'todo', $4)
            "#,
        )
        .bind(msg_id)
        .bind(channel_id)
        .bind(body.lines().next().unwrap_or("Untitled task"))
        .bind(agent_id)
        .execute(&mut *tx)
        .await
        .map_err(to_string)?;
    }

    tx.commit().await.map_err(to_string)?;
    let conversation_thread_root_id = thread_root_id.unwrap_or(msg_id);
    upsert_agent_thread_subscription(
        pool,
        agent_id,
        channel_id,
        conversation_thread_root_id,
        if as_task {
            "task_message"
        } else {
            "agent_message"
        },
        Some(msg_id),
    )
    .await?;
    if !as_task {
        queue_agent_message_mentions(pool, msg_id).await?;
    }
    let _ = notify_ui_refresh(pool, "message").await;
    Ok(msg_id)
}

async fn create_agent_task_thread(
    pool: &PgPool,
    agent_id: Uuid,
    channel_id: Uuid,
    title: &str,
    body: Option<&str>,
    thread_body: Option<&str>,
    assign_self: bool,
    status: Option<&str>,
) -> CommandResult<(i64, Uuid, Option<Uuid>)> {
    let title = title.trim();
    if title.is_empty() {
        return Err("task_create title is required".to_owned());
    }
    let final_status = status
        .map(str::trim)
        .filter(|status| !status.is_empty())
        .unwrap_or(if assign_self { "in_progress" } else { "todo" });
    if !matches!(final_status, "todo" | "in_progress" | "in_review" | "done") {
        return Err(format!("unsupported task status: {final_status}"));
    }
    let root_body = body
        .map(str::trim)
        .filter(|body| !body.is_empty())
        .unwrap_or(title);
    let root_message_id =
        insert_agent_message(pool, agent_id, channel_id, None, root_body, true).await?;
    let task_row = sqlx::query(
        r#"
        update tasks
        set title = $2,
            status = $3,
            assignee_agent_id = case when $4 then $5 else null end,
            updated_at = now()
        where message_id = $1
        returning number
        "#,
    )
    .bind(root_message_id)
    .bind(title)
    .bind(final_status)
    .bind(assign_self)
    .bind(agent_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    let task_number: i64 = task_row.get("number");
    let thread_reply_id = match thread_body.map(str::trim).filter(|body| !body.is_empty()) {
        Some(thread_body) => Some(
            insert_agent_message(
                pool,
                agent_id,
                channel_id,
                Some(root_message_id),
                thread_body,
                false,
            )
            .await?,
        ),
        None => None,
    };
    let _ = notify_ui_refresh(pool, "task_create").await;
    Ok((task_number, root_message_id, thread_reply_id))
}

fn normalize_artifact_kind(kind: &str) -> CommandResult<String> {
    let normalized = kind.trim().to_lowercase().replace('_', "-");
    let normalized = match normalized.as_str() {
        "md" | "markdown" => "markdown",
        "json" => "json",
        "table" | "csv" => "table",
        "chart" | "bar-chart" | "bar_chart" => "chart",
        "diff" | "patch" => "diff",
        "mermaid" | "diagram" => "mermaid",
        "svg" => "svg",
        "html" => "html",
        "text" | "plain" => "text",
        other => {
            return Err(format!(
                "unsupported artifact kind: {other}; supported: markdown, json, table, chart, diff, mermaid, svg, html, text"
            ))
        }
    };
    Ok(normalized.to_owned())
}

async fn create_agent_artifact(
    pool: &PgPool,
    agent_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    kind: &str,
    title: &str,
    summary: Option<&str>,
    content: &str,
    metadata: Option<Value>,
) -> CommandResult<(Uuid, Uuid)> {
    let kind = normalize_artifact_kind(kind)?;
    let title = title.trim();
    if title.is_empty() {
        return Err("artifact_create title is required".to_owned());
    }
    let content = content.trim();
    if content.is_empty() {
        return Err("artifact_create content is required".to_owned());
    }
    let summary = summary
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| content.lines().next().unwrap_or(""))
        .to_owned();
    let summary = compact_chars_middle(&summary, 320).replace('\n', " ");
    let body = if summary.is_empty() {
        format!("Created artifact: {title}")
    } else {
        format!("Created artifact: {title}\n\n{summary}")
    };
    let message_id =
        insert_agent_message(pool, agent_id, channel_id, thread_root_id, &body, false).await?;
    let artifact_id: Uuid = sqlx::query_scalar(
        r#"
        insert into artifacts (
            message_id, channel_id, thread_root_id, creator_agent_id,
            kind, title, summary, content, metadata
        )
        values ($1, $2, $3, $4, $5, $6, $7, $8, $9)
        returning id
        "#,
    )
    .bind(message_id)
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(agent_id)
    .bind(&kind)
    .bind(title)
    .bind(&summary)
    .bind(content)
    .bind(metadata.unwrap_or_else(|| json!({})))
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    if let Ok(artifact) = load_artifact(pool, artifact_id).await {
        let _ = notify_ui_artifact_upsert(pool, &artifact, "artifact_created").await;
    }
    if let Ok(message) = load_message(pool, message_id).await {
        let _ = notify_ui_message_upsert(pool, &message, "artifact_created").await;
    } else {
        let _ = notify_ui_refresh(pool, "artifact_created").await;
    }
    Ok((artifact_id, message_id))
}

fn capped_stream_delta(delta: &str, current_len: usize) -> (String, bool) {
    if current_len >= STREAMING_MESSAGE_BODY_LIMIT {
        return (String::new(), true);
    }
    let remaining = STREAMING_MESSAGE_BODY_LIMIT - current_len;
    let delta_len = delta.chars().count();
    if delta_len <= remaining {
        return (delta.to_owned(), false);
    }

    let marker_len = STREAMING_TRUNCATION_MARKER.chars().count();
    let keep = remaining.saturating_sub(marker_len);
    let mut capped: String = delta.chars().take(keep).collect();
    if remaining >= marker_len {
        capped.push_str(STREAMING_TRUNCATION_MARKER);
    }
    (capped, true)
}

async fn append_streaming_agent_message(
    pool: &PgPool,
    agent_id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    stream_key: &str,
    delta: &str,
) -> CommandResult<Uuid> {
    if stream_key.trim().is_empty() {
        return Err("stream_key is empty".to_owned());
    }
    if delta.is_empty() {
        let existing: Option<Uuid> =
            sqlx::query_scalar("select id from messages where stream_key = $1")
                .bind(stream_key)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?;
        return existing.ok_or_else(|| "streaming message does not exist".to_owned());
    }

    if let Some(row) = sqlx::query(
        "select id, delivery_state, char_length(body) as body_len from messages where stream_key = $1",
    )
    .bind(stream_key)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?
    {
        let message_id: Uuid = row.get("id");
        let delivery_state: String = row.get("delivery_state");
        if delivery_state != "streaming" {
            return Ok(message_id);
        }
        let body_len: i32 = row.get("body_len");
        let (append_delta, truncated) = capped_stream_delta(delta, body_len.max(0) as usize);
        if append_delta.is_empty() && truncated {
            finish_streaming_agent_message(pool, stream_key, "complete").await?;
            return Ok(message_id);
        }
        sqlx::query("update messages set body = body || $2, delivery_state = $3 where id = $1")
            .bind(message_id)
            .bind(&append_delta)
            .bind(if truncated { "complete" } else { "streaming" })
            .execute(pool)
            .await
            .map_err(to_string)?;
        let delivery_state = if truncated { "complete" } else { "streaming" };
        let _ =
            notify_ui_message_delta(pool, message_id, &append_delta, delivery_state, "stream_delta")
                .await;
        if truncated {
            queue_agent_message_mentions(pool, message_id).await?;
        }
        return Ok(message_id);
    }

    let sender = sqlx::query("select display_name, role from agents where id = $1")
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let sender_name: String = sender.get("display_name");
    let sender_role: String = sender.get("role");
    let (initial_body, truncated) = capped_stream_delta(delta, 0);

    let message_id: Uuid = sqlx::query_scalar(
        r#"
        insert into messages (
            channel_id,
            thread_root_id,
            sender_agent_id,
            sender_name,
            sender_role,
            body,
            is_task,
            delivery_state,
            stream_key
        )
        values ($1, $2, $3, $4, $5, $6, false, $7, $8)
        returning id
        "#,
    )
    .bind(channel_id)
    .bind(thread_root_id)
    .bind(agent_id)
    .bind(sender_name)
    .bind(sender_role)
    .bind(initial_body)
    .bind(if truncated { "complete" } else { "streaming" })
    .bind(stream_key)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    if let Ok(message) = load_message(pool, message_id).await {
        let _ = notify_ui_message_upsert(pool, &message, "stream_start").await;
    } else {
        let _ = notify_ui_refresh(pool, "stream_start").await;
    }
    if truncated {
        queue_agent_message_mentions(pool, message_id).await?;
    }
    Ok(message_id)
}

async fn finish_streaming_agent_message(
    pool: &PgPool,
    stream_key: &str,
    delivery_state: &str,
) -> CommandResult<()> {
    let affected = sqlx::query(
        r#"
        update messages
        set delivery_state = $2
        where stream_key = $1
          and delivery_state = 'streaming'
        "#,
    )
    .bind(stream_key)
    .bind(delivery_state)
    .execute(pool)
    .await
    .map_err(to_string)?
    .rows_affected();
    if affected > 0 {
        let message_id: Option<Uuid> =
            sqlx::query_scalar("select id from messages where stream_key = $1")
                .bind(stream_key)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?;
        if let Some(message_id) = message_id {
            if let Ok(message) = load_message(pool, message_id).await {
                let _ = notify_ui_message_upsert(pool, &message, "stream_finish").await;
            } else {
                let _ = notify_ui_refresh(pool, "stream_finish").await;
            }
            if delivery_state == "complete" {
                queue_agent_message_mentions(pool, message_id).await?;
            }
        }
    }
    Ok(())
}

fn silent_reply_reason(body: &str) -> Option<String> {
    let first_line = body.trim().lines().next()?.trim().trim_matches('`').trim();
    let rest = first_line.strip_prefix(SILENT_REPLY_PREFIX)?;
    if !rest.is_empty()
        && !rest.starts_with(':')
        && !rest
            .chars()
            .next()
            .map(char::is_whitespace)
            .unwrap_or(false)
    {
        return None;
    }
    let reason = rest.trim_start_matches(':').trim();
    Some(reason.to_owned())
}

async fn mark_work_item_silent(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    work_item_id: Uuid,
    reason: &str,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        update agent_work_items
        set status = 'silent',
            completed_at = coalesce(completed_at, now()),
            updated_at = now()
        where id = $1
          and status not in ('cancelled', 'failed')
        "#,
    )
    .bind(work_item_id)
    .execute(pool)
    .await
    .map_err(to_string)?;
    notify_ui_work_item_changed(pool, work_item_id, "work_item_silent").await;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "decision",
        "No visible reply needed",
        json!({
            "work_item_id": work_item_id,
            "reason": if reason.trim().is_empty() {
                "Agent judged the message as non-actionable."
            } else {
                reason.trim()
            }
        })
        .to_string(),
    )
    .await?;
    Ok(())
}

async fn mark_run_work_item_silent(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    reason: &str,
) -> CommandResult<()> {
    let work_item_id: Option<Uuid> =
        sqlx::query_scalar("select work_item_id from agent_runs where id = $1 and agent_id = $2")
            .bind(run_id)
            .bind(agent_id)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?
            .flatten();
    if let Some(work_item_id) = work_item_id {
        mark_work_item_silent(pool, agent_id, run_id, work_item_id, reason).await?;
    } else {
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(run_id),
            "decision",
            "No visible reply needed",
            reason.trim(),
        )
        .await?;
    }
    Ok(())
}

async fn maybe_hide_silent_streaming_reply(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    work_item_id: Option<Uuid>,
    stream_key: &str,
) -> CommandResult<bool> {
    let Some(row) = sqlx::query("select id, body from messages where stream_key = $1")
        .bind(stream_key)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?
    else {
        return Ok(false);
    };
    let message_id: Uuid = row.get("id");
    let body: String = row.get("body");
    let Some(reason) = silent_reply_reason(&body) else {
        return Ok(false);
    };

    sqlx::query("delete from messages where id = $1")
        .bind(message_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    let _ = notify_ui_message_delete(pool, message_id, "silent_reply").await;
    if let Some(work_item_id) = work_item_id {
        mark_work_item_silent(pool, agent_id, run_id, work_item_id, &reason).await?;
    } else {
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(run_id),
            "decision",
            "No visible reply needed",
            reason.trim(),
        )
        .await?;
    }
    Ok(true)
}

fn split_streaming_agent_event_lines(body: &str) -> (String, Vec<String>) {
    let mut visible_lines = Vec::new();
    let mut events = Vec::new();
    for line in body.lines() {
        if let Some(json) = extract_agent_event_json(line) {
            events.push(json.to_owned());
        } else {
            visible_lines.push(line);
        }
    }
    (visible_lines.join("\n").trim().to_owned(), events)
}

async fn consume_streaming_agent_control_lines(
    pool: &PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    work_item_id: Option<Uuid>,
    stream_key: &str,
) -> CommandResult<bool> {
    if maybe_hide_silent_streaming_reply(pool, agent_id, run_id, work_item_id, stream_key).await? {
        return Ok(true);
    }

    let Some(row) = sqlx::query("select id, body from messages where stream_key = $1")
        .bind(stream_key)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?
    else {
        return Ok(false);
    };
    let message_id: Uuid = row.get("id");
    let body: String = row.get("body");
    let (visible_body, event_jsons) = split_streaming_agent_event_lines(&body);
    if event_jsons.is_empty() {
        return Ok(false);
    }

    for json in event_jsons {
        match serde_json::from_str::<AgentEvent>(&json).map_err(to_string) {
            Ok(event) => {
                if !claim_agent_event(pool, run_id, &json).await? {
                    continue;
                }
                match handle_agent_event(pool, agent_id, run_id, event).await {
                    Ok(note) => {
                        append_run_log(pool, run_id, format!("[stream-event] {note}\n")).await?;
                        record_agent_activity(
                            pool,
                            Some(agent_id),
                            Some(run_id),
                            "event",
                            "Stream event accepted",
                            note,
                        )
                        .await?;
                    }
                    Err(err) => {
                        append_run_log(pool, run_id, format!("[stream-event] rejected: {err}\n"))
                            .await?;
                        record_agent_activity(
                            pool,
                            Some(agent_id),
                            Some(run_id),
                            "event_error",
                            "Stream event rejected",
                            err,
                        )
                        .await?;
                    }
                }
            }
            Err(err) => {
                append_run_log(pool, run_id, format!("[stream-event] rejected: {err}\n")).await?;
                record_agent_activity(
                    pool,
                    Some(agent_id),
                    Some(run_id),
                    "event_error",
                    "Stream event rejected",
                    err,
                )
                .await?;
            }
        }
    }

    if visible_body.is_empty() {
        sqlx::query("delete from messages where id = $1")
            .bind(message_id)
            .execute(pool)
            .await
            .map_err(to_string)?;
        let _ = notify_ui_message_delete(pool, message_id, "stream_event_consumed").await;
        return Ok(true);
    }

    sqlx::query("update messages set body = $2 where id = $1")
        .bind(message_id)
        .bind(&visible_body)
        .execute(pool)
        .await
        .map_err(to_string)?;
    if let Ok(message) = load_message(pool, message_id).await {
        let _ = notify_ui_message_upsert(pool, &message, "stream_event_consumed").await;
    } else {
        let _ = notify_ui_refresh(pool, "stream_event_consumed").await;
    }
    Ok(false)
}

async fn load_runtime_thread_id(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &str,
) -> CommandResult<Option<String>> {
    let thread_id: Option<String> = sqlx::query_scalar(
        r#"
        select provider_thread_id
        from runtime_sessions
        where agent_id = $1
          and runtime = $2
        "#,
    )
    .bind(agent_id)
    .bind(runtime)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;
    Ok(thread_id.filter(|thread_id| !thread_id.trim().is_empty()))
}

async fn upsert_runtime_thread_id(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &str,
    provider_thread_id: &str,
    status: &str,
) -> CommandResult<()> {
    sqlx::query(
        r#"
        insert into runtime_sessions (agent_id, runtime, provider_thread_id, status)
        values ($1, $2, $3, $4)
        on conflict (agent_id, runtime) do update set
            provider_thread_id = excluded.provider_thread_id,
            status = excluded.status,
            updated_at = now()
        "#,
    )
    .bind(agent_id)
    .bind(runtime)
    .bind(provider_thread_id)
    .bind(status)
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

async fn wait_for_agent_run(
    pool: PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    work_item_id: Option<Uuid>,
    pipe_tasks: Vec<tokio::task::JoinHandle<()>>,
    mut child: tokio::process::Child,
) {
    let result = child.wait().await;
    for task in pipe_tasks {
        let _ = task.await;
    }
    let _ = replay_agent_events_from_run_log_if_needed(&pool, agent_id, run_id).await;
    let current_run_status: Option<String> =
        sqlx::query_scalar("select status from agent_runs where id = $1")
            .bind(run_id)
            .fetch_optional(&pool)
            .await
            .ok()
            .flatten();
    let (run_status, agent_status, exit_code, log_line) = match result {
        _ if current_run_status.as_deref() == Some("failed") => (
            "failed",
            "error",
            None,
            "process exited after runtime error\n".to_owned(),
        ),
        Ok(status) if status.success() => (
            "exited",
            "idle",
            status.code(),
            format!("process exited successfully: {status}\n"),
        ),
        Ok(status) => (
            "stopped",
            "idle",
            status.code(),
            format!("process stopped: {status}\n"),
        ),
        Err(err) => (
            "failed",
            "error",
            None,
            format!("failed while waiting for process: {err}\n"),
        ),
    };

    let _ = sqlx::query(
        r#"
        update agent_runs
        set status = $2,
            exit_code = $3,
            log = right(log || $4, 20000),
            stopped_at = now()
        where id = $1
        "#,
    )
    .bind(run_id)
    .bind(run_status)
    .bind(exit_code)
    .bind(&log_line)
    .execute(&pool)
    .await;
    notify_ui_agent_run_changed(&pool, run_id, "run_finished").await;

    let _ = sqlx::query("update agents set status = $2 where id = $1")
        .bind(agent_id)
        .bind(agent_status)
        .execute(&pool)
        .await;

    if let Some(work_item_id) = work_item_id {
        let current_status: Option<String> =
            sqlx::query_scalar("select status from agent_work_items where id = $1")
                .bind(work_item_id)
                .fetch_optional(&pool)
                .await
                .ok()
                .flatten();
        let work_status = if current_status.as_deref() == Some("cancelling") {
            "cancelled"
        } else if current_status.as_deref() == Some("silent") && run_status == "exited" {
            "silent"
        } else if run_status == "exited" {
            "done"
        } else {
            "failed"
        };
        let _ = sqlx::query(
            r#"
            update agent_work_items
            set status = $2,
                completed_at = now(),
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .bind(work_status)
        .execute(&pool)
        .await;
        notify_ui_work_item_changed(&pool, work_item_id, "work_item_finished").await;
        let _ = record_agent_activity(
            &pool,
            Some(agent_id),
            Some(run_id),
            "dispatch",
            work_status_title(work_status),
            work_item_id.to_string(),
        )
        .await;
    }

    let _ = record_agent_activity(
        &pool,
        Some(agent_id),
        Some(run_id),
        "run",
        format!("Run {run_status}"),
        log_line.trim(),
    )
    .await;
}

fn effective_launch_command(
    launch_command: String,
    _runtime: String,
    _model: String,
    _handle: String,
) -> String {
    if !launch_command.trim().is_empty() {
        return launch_command.trim().to_owned();
    }

    "printf 'LocalSlock placeholder runtime. Configure launch_command to run a real agent.\\n'; sleep 3600"
        .to_owned()
}

fn compact_chars_middle(value: &str, limit: usize) -> String {
    let trimmed = value.trim();
    let chars: Vec<char> = trimmed.chars().collect();
    if chars.len() <= limit {
        return trimmed.to_owned();
    }

    let head_len = limit.saturating_mul(2) / 3;
    let tail_len = limit.saturating_sub(head_len);
    let omitted = chars.len().saturating_sub(head_len + tail_len);
    let head = chars.iter().take(head_len).collect::<String>();
    let tail = chars
        .iter()
        .skip(chars.len().saturating_sub(tail_len))
        .collect::<String>();
    format!("{head}\n\n[... LocalSlock omitted {omitted} chars to keep agent context bounded ...]\n\n{tail}")
}

fn build_work_item_prompt(
    work_item_id: Uuid,
    title: &str,
    context: &str,
    channel_name: Option<&str>,
    task_number: Option<i64>,
    thread_root_id: Option<Uuid>,
    available_agents: &[String],
) -> String {
    let mut lines = vec![
        "Current LocalSlock agent request:".to_owned(),
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
    if !context.trim().is_empty() {
        lines.push("context:".to_owned());
        lines.push(context.trim().to_owned());
    }
    lines.push(
        "Before producing a visible reply, decide whether the latest user message actually needs an agent response. If it is only a greeting, acknowledgement, thanks, emoji, or non-actionable chatter, do not write a normal answer. Instead output exactly `LOCAL_SLOCK_SILENT_REPLY: <short reason>` and nothing else. Visible replies are for questions, requests, reviews, code/task work, decisions, useful status, or meaningful collaboration."
            .to_owned(),
    );
    lines.push(
        r#"Visible reply policy: keep thread messages high-density. Do not narrate every intermediate step, tool call, command output, or file edit in chat. Use visible replies for final results, important decisions, blockers, user questions, and handoffs. Put intermediate progress in activity instead:
LOCAL_SLOCK_EVENT {"type":"activity","kind":"thinking|command|file_edit|tools|acting","title":"<short user-facing status>","detail":"<optional compact detail>"}
Use one activity event per meaningful phase change, not per log line."#
            .to_owned(),
    );
    lines.push(
        r#"You can maintain your persistent workspace memory by emitting standalone control lines:
LOCAL_SLOCK_EVENT {"type":"memory_append","body":"<durable fact, preference, decision, or handoff>"}
LOCAL_SLOCK_EVENT {"type":"memory_compact","body":"<full compact MEMORY.md replacement>"}
Use append for small durable facts. Use compact only when memory is too long or repetitive."#
            .to_owned(),
    );
    lines.push(
        r#"You can update your own public agent profile when your role, specialty, or display identity has genuinely changed:
LOCAL_SLOCK_EVENT {"type":"profile_update","display_name":"<optional>","role":"<optional concise role>","avatar":"<optional short avatar>","description":"<optional capability summary>"}
Use this sparingly. Other agents use your profile when deciding who to collaborate with."#
            .to_owned(),
    );
    lines.push(
        r#"You can create collaboration spaces when the conversation naturally becomes a separate long-lived topic:
LOCAL_SLOCK_EVENT {"type":"channel_create","name":"short-topic","description":"<why this channel exists>","agent_handles":["@OtherAgent"]}
LOCAL_SLOCK_EVENT {"type":"channel_invite","channel":"existing-channel","agent_handles":["@OtherAgent"]}
Only create/invite channels with a clear reason; do not create channels for ordinary short replies."#
            .to_owned(),
    );
    lines.push(
        r#"You can manage reminders by emitting a standalone control line:
LOCAL_SLOCK_EVENT {"type":"reminder_create","when":"<ISO8601 timestamp>","title":"<title>","note":"<optional note>","recurrence":"none|daily|weekly"}
LOCAL_SLOCK_EVENT {"type":"reminder_cancel","reminder_id":"<uuid>"}
Use reminders when the user asks for a future follow-up or when you need to re-check state later. Reminders you create are anchored to this channel/thread by default and will wake you again when due."#
            .to_owned(),
    );
    lines.push(
        r#"You can create explicit tracked tasks only when the conversation is durable work that should be tracked globally:
LOCAL_SLOCK_EVENT {"type":"task_create","channel_id":"<channel uuid>","title":"<short task title>","body":"<root task message>","thread_body":"<first execution update in the task thread>","assign_self":true,"status":"in_progress"}
This creates a root task message in the channel and opens its execution thread with thread_body. Do not create tasks for greetings, small clarifications, or ordinary chat. For normal follow-up, reply in the current thread with a message event instead."#
            .to_owned(),
    );
    lines.push(
        "If a message includes attachments, use the attachment_id/local_path shown in context. For image attachments, inspect local_path with your runtime's file or vision support before answering visual UI questions. You can also run the read-only context tool: attachment-info --attachment-id <uuid>."
            .to_owned(),
    );
    lines.push(
        "For cross-agent context, run the read-only context tool: agent-inspect --target @handle. Use it before delegating when you need another agent's role, recent runs, recent requests, or current activity."
            .to_owned(),
    );
    lines.push(
        r#"You can create structured artifacts for dense data or long outputs:
LOCAL_SLOCK_EVENT {"type":"artifact_create","channel_id":"<channel uuid>","thread_root_id":"<optional uuid>","kind":"markdown|json|table|chart|diff|svg|html|text","title":"<short title>","summary":"<short chat summary>","content":"<full artifact content>","metadata":{}}
Use artifacts for reports, tables, diffs, SVG diagrams, JSON, and long analysis. Keep the visible chat summary short; put the detailed content in the artifact. Prefer SVG for architecture diagrams because Mermaid is stored as source text only."#
            .to_owned(),
    );
    lines.push(
        "When you finish, write results back with LOCAL_SLOCK_EVENT lines. Only update task status when this request is tied to an explicit task number."
            .to_owned(),
    );
    lines.join("\n")
}

async fn load_channel_agent_roster(
    pool: &PgPool,
    channel_id: Option<Uuid>,
    current_agent_id: Uuid,
) -> CommandResult<Vec<String>> {
    let Some(channel_id) = channel_id else {
        return Ok(vec![]);
    };
    let rows = sqlx::query(
        r#"
        select a.handle, a.display_name, a.runtime, a.model, a.status
        from channels c
        join channel_members cm on cm.channel_id = c.id
        join agents a on a.id = cm.agent_id
        where c.id = $1
          and c.kind = 'channel'
          and a.id <> $2
        order by lower(a.handle)
        "#,
    )
    .bind(channel_id)
    .bind(current_agent_id)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
        .into_iter()
        .map(|row| {
            let handle: String = row.get("handle");
            let display_name: String = row.get("display_name");
            let runtime: String = row.get("runtime");
            let model: String = row.get("model");
            let status: String = row.get("status");
            format!("@{handle} - {display_name} - {runtime}/{model} - {status}")
        })
        .collect())
}

fn load_agent_memory_context(working_directory: &str) -> CommandResult<Option<String>> {
    let working_directory = working_directory.trim();
    if working_directory.is_empty() {
        return Ok(None);
    }
    let memory_path = PathBuf::from(working_directory).join("MEMORY.md");
    if !memory_path.exists() {
        return Ok(None);
    }
    let metadata = fs::metadata(&memory_path).map_err(to_string)?;
    if !metadata.is_file() {
        return Ok(None);
    }
    let memory = fs::read_to_string(&memory_path).map_err(to_string)?;
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

fn ensure_agent_workspace(working_directory: &str, handle: &str) -> CommandResult<()> {
    let working_directory = working_directory.trim();
    if working_directory.is_empty() {
        return Ok(());
    }
    let workspace = PathBuf::from(working_directory);
    fs::create_dir_all(&workspace).map_err(to_string)?;
    let memory_path = workspace.join("MEMORY.md");
    if memory_path.exists() {
        return Ok(());
    }
    let template = format!(
        "# @{handle}\n\n## Role\nLocalSlock agent.\n\n## Persistent Context\n- Add durable facts, user preferences, project notes, and handoff context here.\n- LocalSlock injects this file into the warm runtime standing prompt with a bounded context budget.\n",
    );
    fs::write(memory_path, template).map_err(to_string)?;
    Ok(())
}

fn prepend_memory_context(prompt: String, memory_context: Option<&str>) -> String {
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
         Each new turn contains the latest inbox item plus a bounded context snapshot. Do not assume it is an exhaustive transcript; rely on the active runtime session and use the history/search tool when older context is needed.\n\
         For read-only LocalSlock history lookup, run: `\"$LOCAL_SLOCK_CONTEXT_TOOL\" --agent-context-tool history-read --target \"#channel[:thread_id]\" --limit 20`.\n\
         For read-only message search, run: `\"$LOCAL_SLOCK_CONTEXT_TOOL\" --agent-context-tool message-search --query \"text\" --target \"#channel\" --limit 20`. Omit --target to search all local messages.\n\
         For attachment details, run: `\"$LOCAL_SLOCK_CONTEXT_TOOL\" --agent-context-tool attachment-info --attachment-id \"<uuid>\"`; image attachments expose a local_path you can inspect with your runtime's file/vision support.\n\
         For cross-agent introspection, run: `\"$LOCAL_SLOCK_CONTEXT_TOOL\" --agent-context-tool agent-inspect --target \"@handle\"` to see an agent profile, recent runs, requests, and activity.\n\
         For structured outputs, emit LOCAL_SLOCK_EVENT artifact_create with kind markdown/json/table/chart/diff/svg/html/text; keep chat concise and put long data in the artifact. Prefer SVG for architecture diagrams; mermaid is stored as source text only.\n\
         Before replying, decide whether a visible response is useful; for greetings, acknowledgements, thanks, emoji, or non-actionable chatter, output exactly LOCAL_SLOCK_SILENT_REPLY: <short reason> and nothing else.\n\
         Keep thread messages high-density: do not narrate every intermediate step, tool call, command output, or file edit in chat. Use visible replies for final results, important decisions, blockers, user questions, and handoffs.\n\
         For intermediate progress, emit standalone LOCAL_SLOCK_EVENT activity control lines such as {{\"type\":\"activity\",\"kind\":\"command\",\"title\":\"Running tests\",\"detail\":\"cargo test\"}}; LocalSlock records them in the agent activity feed and hides the control line from chat.\n\
         You may print standalone LOCAL_SLOCK_EVENT reminder_create/reminder_cancel, memory_append/memory_compact, profile_update, channel_create/channel_invite, artifact_create, and usage control lines; LocalSlock consumes and hides those lines. Do not print LOCAL_SLOCK_EVENT message/task lines unless explicitly asked to debug the legacy runtime path.\n\
         Keep user-visible replies concise and include concrete results or blockers."
    );
    if let Some(memory_context) = memory_context.filter(|context| !context.trim().is_empty()) {
        prompt.push_str("\n\n");
        prompt.push_str(memory_context.trim());
    }
    prompt
}

fn build_codex_streaming_prompt(legacy_prompt: &str) -> String {
    if legacy_prompt.trim().is_empty() {
        return "No current LocalSlock agent request is assigned. Reply with a short ready status."
            .to_owned();
    }
    legacy_prompt.replace(
        "When you finish, write results back with LOCAL_SLOCK_EVENT lines. Only update task status when this request is tied to an explicit task number.",
        "Reply normally only when a visible response is useful. LocalSlock will stream your assistant text into the correct channel/thread automatically. If the latest user message is only a greeting, acknowledgement, thanks, emoji, or non-actionable chatter, output exactly `LOCAL_SLOCK_SILENT_REPLY: <short reason>` and nothing else. Keep visible thread messages high-density: final results, decisions, blockers, user questions, and handoffs only. Do not narrate every intermediate step in chat. For progress, emit standalone LOCAL_SLOCK_EVENT activity control lines like {\"type\":\"activity\",\"kind\":\"command\",\"title\":\"Running tests\",\"detail\":\"cargo test\"}; LocalSlock consumes and records them in the activity feed. You may emit standalone LOCAL_SLOCK_EVENT reminder_create/reminder_cancel, memory_append/memory_compact, channel_create/channel_invite, and usage control lines; LocalSlock will consume and hide those control lines. Do not emit LOCAL_SLOCK_EVENT message/task lines in this Codex JSON streaming mode.",
    )
}

fn codex_developer_instructions(handle: &str, memory_context: Option<&str>) -> String {
    build_runtime_standing_prompt(
        handle,
        "LocalSlock is connected to Codex through the official app-server JSON protocol and streams your assistant text into chat automatically.",
        memory_context,
    )
}

fn claude_system_prompt(handle: &str, memory_context: Option<&str>) -> String {
    build_runtime_standing_prompt(
        handle,
        "LocalSlock is connected to Claude through Claude Code stream-json and streams your assistant text into chat automatically.",
        memory_context,
    )
}

fn configure_agent_context_tool_env(command: &mut Command) {
    if let Ok(exe_path) = env::current_exe() {
        command.env(LOCAL_SLOCK_CONTEXT_TOOL_ENV, exe_path);
    }
    command.env("LOCAL_SLOCK_DATABASE_URL", db_url());
}

fn codex_stream_key(run_id: Uuid, item_id: &str) -> String {
    format!("{run_id}:{item_id}")
}

async fn codex_write_json(
    stdin: &mut tokio::process::ChildStdin,
    value: Value,
) -> CommandResult<()> {
    let mut line = serde_json::to_vec(&value).map_err(to_string)?;
    line.push(b'\n');
    stdin.write_all(&line).await.map_err(to_string)?;
    stdin.flush().await.map_err(to_string)?;
    Ok(())
}

fn codex_request_error(value: &Value) -> Option<String> {
    value.get("error").map(|error| {
        error
            .get("message")
            .and_then(Value::as_str)
            .map(str::to_owned)
            .unwrap_or_else(|| error.to_string())
    })
}

fn codex_thread_id_from_response(value: &Value) -> Option<String> {
    value
        .pointer("/result/thread/id")
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn codex_turn_id_from_value(value: &Value) -> Option<String> {
    value
        .pointer("/result/turn/id")
        .or_else(|| value.pointer("/params/turn/id"))
        .or_else(|| value.pointer("/params/turnId"))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn codex_item_type(value: &Value) -> Option<&str> {
    value.pointer("/params/item/type").and_then(Value::as_str)
}

fn codex_item_id(value: &Value) -> Option<&str> {
    value.pointer("/params/item/id").and_then(Value::as_str)
}

fn codex_item_summary(value: &Value) -> String {
    let Some(item) = value.pointer("/params/item") else {
        return "item".to_owned();
    };
    match item.get("type").and_then(Value::as_str).unwrap_or("item") {
        "commandExecution" => item
            .get("command")
            .and_then(Value::as_str)
            .unwrap_or("shell command")
            .to_owned(),
        "mcpToolCall" => format!(
            "{}.{}",
            item.get("server").and_then(Value::as_str).unwrap_or("mcp"),
            item.get("tool").and_then(Value::as_str).unwrap_or("tool")
        ),
        "dynamicToolCall" => format!(
            "{}{}",
            item.get("namespace")
                .and_then(Value::as_str)
                .map(|namespace| format!("{namespace}."))
                .unwrap_or_default(),
            item.get("tool").and_then(Value::as_str).unwrap_or("tool")
        ),
        "webSearch" => item
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or("web search")
            .to_owned(),
        "fileChange" => "File change".to_owned(),
        "reasoning" => "Thinking".to_owned(),
        "agentMessage" => "Writing response".to_owned(),
        other => other.to_owned(),
    }
}

fn first_nonempty_item_value<'a>(item: &'a Value, fields: &[&str]) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|field| item.pointer(field).or_else(|| item.get(*field)))
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
}

fn codex_item_file_summary(item: &Value) -> String {
    let path = first_nonempty_item_value(
        item,
        &[
            "path",
            "file",
            "filePath",
            "filename",
            "/path",
            "/file",
            "/filePath",
            "/changes/0/path",
            "/changes/0/file",
        ],
    )
    .unwrap_or("file");
    let operation = first_nonempty_item_value(
        item,
        &["operation", "action", "change", "/operation", "/action"],
    )
    .unwrap_or("edit");
    json!({ "file": path, "operation": operation }).to_string()
}

fn codex_item_started_activity(value: &Value) -> (&'static str, &'static str, String) {
    let Some(item) = value.pointer("/params/item") else {
        return ("activity", "Codex activity", "item".to_owned());
    };
    match item.get("type").and_then(Value::as_str).unwrap_or("item") {
        "reasoning" => ("thinking", "Thinking", "Thinking".to_owned()),
        "commandExecution" => (
            "command",
            "Running command",
            json!({ "command": codex_item_summary(value) }).to_string(),
        ),
        "fileChange" => ("file_edit", "Editing file", codex_item_file_summary(item)),
        "mcpToolCall" | "dynamicToolCall" | "webSearch" => {
            ("tools", "Using tool", codex_item_summary(value))
        }
        "agentMessage" => ("acting", "Writing response", "Writing response".to_owned()),
        _ => ("activity", "Codex activity", codex_item_summary(value)),
    }
}

fn first_nonempty_item_string<'a>(item: &'a Value, fields: &[&str]) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|field| item.get(*field).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn codex_tool_completion_activity(value: &Value) -> Option<(&'static str, &'static str, String)> {
    let item = value.pointer("/params/item")?;
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("item");
    if !matches!(
        item_type,
        "commandExecution" | "mcpToolCall" | "dynamicToolCall" | "webSearch" | "fileChange"
    ) {
        return None;
    }

    let (kind, title, mut metadata) = match item_type {
        "commandExecution" => (
            "command",
            "Command finished",
            json!({ "command": codex_item_summary(value) }),
        ),
        "fileChange" => (
            "file_edit",
            "File edit finished",
            serde_json::from_str(&codex_item_file_summary(item))
                .unwrap_or_else(|_| json!({ "file": "file" })),
        ),
        _ => (
            "tools",
            "Tool completed",
            json!({ "tool": codex_item_summary(value) }),
        ),
    };

    if let Some(object) = metadata.as_object_mut() {
        if let Some(exit_code) = item.get("exitCode").and_then(Value::as_i64) {
            object.insert("exit_code".to_owned(), json!(exit_code));
        }
        if let Some(status) = first_nonempty_item_string(item, &["status", "state"]) {
            object.insert("status".to_owned(), json!(status));
        }
        if let Some(output) =
            first_nonempty_item_string(item, &["output", "stdout", "stderr", "result", "error"])
        {
            object.insert("output".to_owned(), json!(truncate_activity_detail(output)));
        }
    }

    Some((kind, title, metadata.to_string()))
}

fn effective_codex_cwd(working_directory: &str) -> CommandResult<String> {
    if working_directory.trim().is_empty() {
        Ok(env::current_dir()
            .map_err(to_string)?
            .to_string_lossy()
            .to_string())
    } else {
        Ok(working_directory.trim().to_owned())
    }
}

fn codex_model_value(model: &str) -> Value {
    if model.trim().is_empty() {
        Value::Null
    } else {
        json!(model.trim())
    }
}

fn build_claude_streaming_prompt(legacy_prompt: &str) -> String {
    if legacy_prompt.trim().is_empty() {
        return "No current LocalSlock agent request is assigned. Reply with a short ready status."
            .to_owned();
    }
    legacy_prompt.replace(
        "When you finish, write results back with LOCAL_SLOCK_EVENT lines. Only update task status when this request is tied to an explicit task number.",
        "Reply normally only when a visible response is useful. LocalSlock will stream your assistant text into the correct channel/thread automatically. If the latest user message is only a greeting, acknowledgement, thanks, emoji, or non-actionable chatter, output exactly `LOCAL_SLOCK_SILENT_REPLY: <short reason>` and nothing else. Keep visible thread messages high-density: final results, decisions, blockers, user questions, and handoffs only. Do not narrate every intermediate step in chat. For progress, emit standalone LOCAL_SLOCK_EVENT activity control lines like {\"type\":\"activity\",\"kind\":\"command\",\"title\":\"Running tests\",\"detail\":\"cargo test\"}; LocalSlock consumes and records them in the activity feed. You may emit standalone LOCAL_SLOCK_EVENT reminder_create/reminder_cancel, memory_append/memory_compact, channel_create/channel_invite, and usage control lines; LocalSlock will consume and hide those control lines. Do not emit LOCAL_SLOCK_EVENT message/task lines in this Claude stream-json mode.",
    )
}

fn claude_stream_key(run_id: Uuid) -> String {
    format!("{run_id}:claude-assistant")
}

fn claude_text_delta(value: &Value) -> Option<&str> {
    if value.get("type").and_then(Value::as_str) != Some("stream_event") {
        return None;
    }
    if value.pointer("/event/delta/type").and_then(Value::as_str) != Some("text_delta") {
        return None;
    }
    value.pointer("/event/delta/text").and_then(Value::as_str)
}

fn claude_session_id(value: &Value) -> Option<&str> {
    value.get("session_id").and_then(Value::as_str)
}

fn claude_message_text(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("assistant") {
        return None;
    }
    let content = value.pointer("/message/content")?.as_array()?;
    let text = content
        .iter()
        .filter_map(|block| {
            if block.get("type").and_then(Value::as_str) == Some("text") {
                block.get("text").and_then(Value::as_str)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("");
    (!text.trim().is_empty()).then_some(text)
}

fn claude_result_text(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("result") {
        return None;
    }
    value
        .get("result")
        .and_then(Value::as_str)
        .filter(|text| !text.trim().is_empty())
        .map(str::to_owned)
}

fn claude_result_error(value: &Value) -> Option<String> {
    if value.get("type").and_then(Value::as_str) != Some("result") {
        return None;
    }
    let is_error = value
        .get("is_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let api_error_status = value
        .get("api_error_status")
        .and_then(Value::as_str)
        .filter(|status| !status.trim().is_empty());
    if !is_error && api_error_status.is_none() {
        return None;
    }
    value
        .get("error")
        .and_then(Value::as_str)
        .or(api_error_status)
        .or_else(|| value.get("result").and_then(Value::as_str))
        .map(str::to_owned)
        .or_else(|| Some("Claude stream-json result reported an error".to_owned()))
}

fn claude_stream_event_activity(value: &Value) -> Option<(&'static str, &'static str, String)> {
    match value.get("type").and_then(Value::as_str)? {
        "system" => match value.get("subtype").and_then(Value::as_str) {
            Some("init") => Some(("run", "Runtime ready", "Claude stream connected".to_owned())),
            Some("api_retry") => Some((
                "run_error",
                "Retrying request",
                truncate_activity_detail(&value.to_string()),
            )),
            Some(_) => None,
            None => None,
        },
        "rate_limit_event" => {
            let status = value
                .pointer("/rate_limit_info/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            if status.eq_ignore_ascii_case("allowed") {
                None
            } else {
                Some((
                    "run_error",
                    "Waiting on rate limit",
                    format!("status={status}"),
                ))
            }
        }
        "stream_event" => {
            let event_type = value.pointer("/event/type").and_then(Value::as_str)?;
            match event_type {
                "content_block_start" => {
                    let block_type = value
                        .pointer("/event/content_block/type")
                        .and_then(Value::as_str)
                        .unwrap_or("content");
                    if block_type == "tool_use" {
                        let name = value
                            .pointer("/event/content_block/name")
                            .and_then(Value::as_str)
                            .unwrap_or("tool");
                        match name {
                            "Bash" => Some((
                                "command",
                                "Running command",
                                json!({ "tool": name }).to_string(),
                            )),
                            "Edit" | "MultiEdit" | "Write" | "NotebookEdit" => Some((
                                "file_edit",
                                "Editing file",
                                json!({ "tool": name }).to_string(),
                            )),
                            _ => Some(("tools", "Using tool", name.to_owned())),
                        }
                    } else if block_type == "thinking" {
                        Some(("thinking", "Thinking", "Claude is thinking".to_owned()))
                    } else {
                        None
                    }
                }
                "content_block_stop" | "message_stop" => None,
                _ => None,
            }
        }
        _ => None,
    }
}

async fn streaming_message_exists(pool: &PgPool, stream_key: &str) -> CommandResult<bool> {
    let exists: bool =
        sqlx::query_scalar("select exists(select 1 from messages where stream_key = $1)")
            .bind(stream_key)
            .fetch_one(pool)
            .await
            .map_err(to_string)?;
    Ok(exists)
}

async fn get_or_spawn_warm_claude_runtime(
    pool: &PgPool,
    registry: &WarmClaudeRegistry,
    agent_id: Uuid,
    handle: &str,
    model: &str,
    working_directory: &str,
    memory_context: Option<&str>,
) -> CommandResult<Arc<WarmClaudeRuntime>> {
    if let Some(runtime) = {
        let runtimes = registry.runtimes.lock().await;
        runtimes.get(&agent_id).cloned()
    } {
        if runtime.state.lock().await.alive {
            return Ok(runtime);
        }
        registry.runtimes.lock().await.remove(&agent_id);
    }

    let runtime = spawn_warm_claude_runtime(
        pool,
        registry.clone(),
        agent_id,
        handle,
        model,
        working_directory,
        memory_context,
    )
    .await?;
    registry
        .runtimes
        .lock()
        .await
        .insert(agent_id, runtime.clone());
    Ok(runtime)
}

fn claude_streaming_command_text(model: &str) -> String {
    format!(
        "claude -p --model {model} --output-format stream-json --input-format stream-json --include-partial-messages --verbose --permission-mode bypassPermissions"
    )
}

fn claude_user_input(prompt: &str) -> Value {
    json!({
        "type": "user",
        "message": {
            "role": "user",
            "content": [{
                "type": "text",
                "text": prompt
            }]
        }
    })
}

async fn claude_write_input(
    stdin: &mut tokio::process::ChildStdin,
    value: Value,
) -> CommandResult<()> {
    let mut line = serde_json::to_vec(&value).map_err(to_string)?;
    line.push(b'\n');
    stdin.write_all(&line).await.map_err(to_string)?;
    stdin.flush().await.map_err(to_string)
}

async fn spawn_warm_claude_runtime(
    pool: &PgPool,
    registry: WarmClaudeRegistry,
    agent_id: Uuid,
    handle: &str,
    model: &str,
    working_directory: &str,
    memory_context: Option<&str>,
) -> CommandResult<Arc<WarmClaudeRuntime>> {
    let model = if model.trim().is_empty() {
        "sonnet".to_owned()
    } else {
        model.trim().to_owned()
    };
    let mut command = Command::new("claude");
    command
        .arg("-p")
        .arg("--system-prompt")
        .arg(claude_system_prompt(handle, memory_context))
        .arg("--model")
        .arg(&model)
        .arg("--output-format")
        .arg("stream-json")
        .arg("--input-format")
        .arg("stream-json")
        .arg("--include-partial-messages")
        .arg("--verbose")
        .arg("--permission-mode")
        .arg("bypassPermissions");
    command.env("LOCAL_SLOCK_AGENT_ID", agent_id.to_string());
    command.env("LOCAL_SLOCK_AGENT_HANDLE", handle);
    configure_agent_context_tool_env(&mut command);
    #[cfg(unix)]
    command.process_group(0);
    if !working_directory.is_empty() {
        command.current_dir(working_directory);
    }
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            sqlx::query("update agents set status = 'error' where id = $1")
                .bind(agent_id)
                .execute(pool)
                .await
                .map_err(to_string)?;
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "run_error",
                "Claude warm stream-json failed to start",
                err.to_string(),
            )
            .await?;
            return Err(err.to_string());
        }
    };

    let pid = child.id().map(|id| id as i32);
    let Some(stdin) = child.stdin.take() else {
        return Err("Claude stream-json stdin unavailable".to_owned());
    };
    let Some(stdout) = child.stdout.take() else {
        return Err("Claude stream-json stdout unavailable".to_owned());
    };
    let stderr = child.stderr.take();

    let runtime = Arc::new(WarmClaudeRuntime {
        stdin: AsyncMutex::new(stdin),
        state: AsyncMutex::new(WarmClaudeState {
            alive: true,
            active: None,
            session_id: None,
            last_activity: Instant::now(),
        }),
        pid,
    });

    upsert_runtime_thread_id(
        pool,
        agent_id,
        "claude",
        &pid.map(|pid| format!("pid:{pid}"))
            .unwrap_or_else(|| "warming".to_owned()),
        "idle",
    )
    .await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        None,
        "run",
        "Claude warm stream-json ready",
        pid.map(|pid| format!("pid={pid}"))
            .unwrap_or_else(|| "pid unavailable".to_owned()),
    )
    .await?;

    tokio::spawn(claude_warm_stdout_reader(
        pool.clone(),
        registry.clone(),
        agent_id,
        runtime.clone(),
        stdout,
    ));
    if let Some(stderr) = stderr {
        tokio::spawn(claude_warm_stderr_reader(
            pool.clone(),
            agent_id,
            runtime.clone(),
            stderr,
        ));
    }
    tokio::spawn(wait_for_warm_claude_process(
        pool.clone(),
        registry,
        agent_id,
        runtime.clone(),
        child,
    ));
    tokio::spawn(claude_warm_idle_reaper(
        pool.clone(),
        agent_id,
        runtime.clone(),
    ));

    Ok(runtime)
}

async fn get_or_spawn_warm_codex_runtime(
    pool: &PgPool,
    registry: &WarmCodexRegistry,
    agent_id: Uuid,
    handle: &str,
    model: &str,
    working_directory: &str,
    memory_context: Option<&str>,
) -> CommandResult<Arc<WarmCodexRuntime>> {
    if let Some(runtime) = {
        let runtimes = registry.runtimes.lock().await;
        runtimes.get(&agent_id).cloned()
    } {
        if runtime.state.lock().await.alive {
            return Ok(runtime);
        }
        registry.runtimes.lock().await.remove(&agent_id);
    }

    let runtime = spawn_warm_codex_runtime(
        pool,
        registry.clone(),
        agent_id,
        handle,
        model,
        working_directory,
        memory_context,
    )
    .await?;
    registry
        .runtimes
        .lock()
        .await
        .insert(agent_id, runtime.clone());
    Ok(runtime)
}

async fn spawn_warm_codex_runtime(
    pool: &PgPool,
    registry: WarmCodexRegistry,
    agent_id: Uuid,
    handle: &str,
    model: &str,
    working_directory: &str,
    memory_context: Option<&str>,
) -> CommandResult<Arc<WarmCodexRuntime>> {
    let cwd = effective_codex_cwd(working_directory)?;
    let mut command = Command::new("/bin/zsh");
    command
        .arg("-lc")
        .arg("exec codex app-server --listen stdio://");
    command.env("LOCAL_SLOCK_AGENT_ID", agent_id.to_string());
    command.env("LOCAL_SLOCK_AGENT_HANDLE", handle);
    configure_agent_context_tool_env(&mut command);
    #[cfg(unix)]
    command.process_group(0);
    command.current_dir(&cwd);
    command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            sqlx::query("update agents set status = 'error' where id = $1")
                .bind(agent_id)
                .execute(pool)
                .await
                .map_err(to_string)?;
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "run_error",
                "Codex warm app-server failed to start",
                err.to_string(),
            )
            .await?;
            return Err(err.to_string());
        }
    };

    let pid = child.id().map(|id| id as i32);
    let Some(mut stdin) = child.stdin.take() else {
        return Err("codex app-server stdin unavailable".to_owned());
    };
    let Some(stdout) = child.stdout.take() else {
        return Err("codex app-server stdout unavailable".to_owned());
    };
    let stderr = child.stderr.take();
    let mut reader = BufReader::new(stdout);
    let developer_instructions = codex_developer_instructions(handle, memory_context);
    let model_value = codex_model_value(model);
    let mut next_request_id = 1_i64;
    let initialize_id = next_request_id;
    next_request_id += 1;
    let mut thread_request_id = next_request_id;
    next_request_id += 1;

    codex_write_json(
        &mut stdin,
        json!({
            "method": "initialize",
            "id": initialize_id,
            "params": {
                "clientInfo": {
                    "name": "localslock",
                    "title": "LocalSlock",
                    "version": env!("CARGO_PKG_VERSION")
                },
                "capabilities": {
                    "experimentalApi": true
                }
            }
        }),
    )
    .await?;
    codex_write_json(&mut stdin, json!({ "method": "initialized" })).await?;

    let existing_thread_id = load_runtime_thread_id(pool, agent_id, "codex").await?;
    let mut attempted_resume = existing_thread_id.is_some();
    if let Some(thread_id) = existing_thread_id {
        codex_write_json(
            &mut stdin,
            json!({
                "method": "thread/resume",
                "id": thread_request_id,
                "params": {
                    "threadId": thread_id.clone(),
                    "model": model_value.clone(),
                    "cwd": cwd.clone(),
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                    "developerInstructions": developer_instructions.clone(),
                    "persistExtendedHistory": true
                }
            }),
        )
        .await?;
    } else {
        codex_write_json(
            &mut stdin,
            json!({
                "method": "thread/start",
                "id": thread_request_id,
                "params": {
                    "model": model_value.clone(),
                    "cwd": cwd.clone(),
                    "approvalPolicy": "never",
                    "sandbox": "danger-full-access",
                    "developerInstructions": developer_instructions.clone(),
                    "experimentalRawEvents": false,
                    "persistExtendedHistory": true
                }
            }),
        )
        .await?;
    }

    let thread_id = loop {
        let mut line = String::new();
        let bytes = reader.read_line(&mut line).await.map_err(to_string)?;
        if bytes == 0 {
            return Err("codex app-server closed during warm initialization".to_owned());
        }
        let line = line.trim_end_matches(['\r', '\n']);
        let value: Value = serde_json::from_str(line).map_err(to_string)?;
        if value.get("id").and_then(Value::as_i64) == Some(thread_request_id) {
            if let Some(error) = codex_request_error(&value) {
                if attempted_resume {
                    attempted_resume = false;
                    thread_request_id = next_request_id;
                    next_request_id += 1;
                    codex_write_json(
                        &mut stdin,
                        json!({
                            "method": "thread/start",
                            "id": thread_request_id,
                            "params": {
                                "model": model_value.clone(),
                                "cwd": cwd.clone(),
                                "approvalPolicy": "never",
                                "sandbox": "danger-full-access",
                                "developerInstructions": developer_instructions.clone(),
                                "experimentalRawEvents": false,
                                "persistExtendedHistory": true
                            }
                        }),
                    )
                    .await?;
                    record_agent_activity(
                        pool,
                        Some(agent_id),
                        None,
                        "run",
                        "Codex thread resume failed; starting new thread",
                        error,
                    )
                    .await?;
                    continue;
                }
                return Err(format!("codex thread request failed: {error}"));
            }
            let Some(thread_id) = codex_thread_id_from_response(&value) else {
                return Err("codex thread response missing thread id".to_owned());
            };
            break thread_id;
        }
    };

    upsert_runtime_thread_id(pool, agent_id, "codex", &thread_id, "idle").await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        None,
        "run",
        "Codex warm app-server ready",
        pid.map(|pid| format!("pid={pid}, thread_id={thread_id}"))
            .unwrap_or_else(|| format!("thread_id={thread_id}")),
    )
    .await?;

    let runtime = Arc::new(WarmCodexRuntime {
        stdin: AsyncMutex::new(stdin),
        state: AsyncMutex::new(WarmCodexState {
            alive: true,
            active: None,
            next_request_id,
            last_activity: Instant::now(),
        }),
        thread_id,
        pid,
    });

    tokio::spawn(codex_warm_stdout_reader(
        pool.clone(),
        registry.clone(),
        agent_id,
        runtime.clone(),
        reader,
    ));
    if let Some(stderr) = stderr {
        tokio::spawn(codex_warm_stderr_reader(
            pool.clone(),
            agent_id,
            runtime.clone(),
            stderr,
        ));
    }
    tokio::spawn(wait_for_warm_codex_process(
        pool.clone(),
        registry,
        agent_id,
        runtime.clone(),
        child,
    ));
    tokio::spawn(codex_warm_idle_reaper(
        pool.clone(),
        agent_id,
        runtime.clone(),
    ));

    Ok(runtime)
}

async fn run_supervisor() -> CommandResult<()> {
    let database_url = db_url();
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(&database_url)
        .await
        .map_err(to_string)?;
    migrate(&pool).await.map_err(to_string)?;

    let acquired: bool = sqlx::query_scalar("select pg_try_advisory_lock($1)")
        .bind(SUPERVISOR_LOCK_ID)
        .fetch_one(&pool)
        .await
        .map_err(to_string)?;

    if !acquired {
        return Ok(());
    }

    mark_orphaned_agent_runs(&pool).await?;
    let codex_registry = WarmCodexRegistry::default();
    let claude_registry = WarmClaudeRegistry::default();
    let mut listener = PgListener::connect(&database_url)
        .await
        .map_err(to_string)?;
    listener
        .listen(SUPERVISOR_WAKE_CHANNEL)
        .await
        .map_err(to_string)?;
    let mut last_command_cleanup = Instant::now() - Duration::from_secs(3600);

    loop {
        write_supervisor_heartbeat(&pool).await?;
        if last_command_cleanup.elapsed() >= Duration::from_secs(3600) {
            cleanup_supervisor_commands(&pool).await?;
            last_command_cleanup = Instant::now();
        }
        schedule_queued_work_items(&pool, &codex_registry).await?;
        let mut processed_command = false;
        while let Some(command) = claim_next_supervisor_command(&pool).await? {
            processed_command = true;
            let command_id = command.id;
            let result =
                process_supervisor_command(&pool, &codex_registry, &claude_registry, command).await;
            finish_supervisor_command(&pool, command_id, result.err()).await?;
        }
        if processed_command {
            continue;
        }
        tokio::select! {
            _ = sleep(Duration::from_secs(2)) => {}
            notification = listener.recv() => {
                if let Err(err) = notification {
                    eprintln!("LocalSlock supervisor wake listener disconnected: {err}");
                    listener = PgListener::connect(&database_url).await.map_err(to_string)?;
                    listener.listen(SUPERVISOR_WAKE_CHANNEL).await.map_err(to_string)?;
                }
            }
        }
    }
}

async fn active_codex_turn_surface(
    registry: &WarmCodexRegistry,
    agent_id: Uuid,
) -> Option<(Option<Uuid>, Option<Uuid>, bool)> {
    let runtime = {
        let runtimes = registry.runtimes.lock().await;
        runtimes.get(&agent_id).cloned()
    }?;
    let state = runtime.state.lock().await;
    let active = state.active.as_ref()?;
    Some((
        active.channel_id,
        active.thread_root_id,
        active.turn_id.is_some() && !active.steer_disabled,
    ))
}

async fn same_codex_surface(
    pool: &PgPool,
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
    active_channel_id: Option<Uuid>,
    active_thread_root_id: Option<Uuid>,
) -> CommandResult<bool> {
    if channel_id.is_none() || channel_id != active_channel_id {
        return Ok(false);
    }
    let Some(channel_id) = channel_id else {
        return Ok(false);
    };
    let kind: Option<String> = sqlx::query_scalar("select kind from channels where id = $1")
        .bind(channel_id)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?;
    if kind.as_deref() == Some("dm") {
        return Ok(true);
    }
    Ok(thread_root_id == active_thread_root_id)
}

async fn should_schedule_queued_work_item(
    pool: &PgPool,
    registry: &WarmCodexRegistry,
    agent_id: Uuid,
    channel_id: Option<Uuid>,
    thread_root_id: Option<Uuid>,
) -> CommandResult<bool> {
    let runtime = agent_runtime(pool, agent_id).await?;
    if !runtime
        .as_deref()
        .is_some_and(|runtime| runtime.eq_ignore_ascii_case("codex"))
    {
        return Ok(!agent_has_active_or_pending_start(pool, agent_id).await?);
    }

    let Some((active_channel_id, active_thread_root_id, has_turn_id)) =
        active_codex_turn_surface(registry, agent_id).await
    else {
        return Ok(true);
    };
    if !has_turn_id {
        return Ok(false);
    }
    same_codex_surface(
        pool,
        channel_id,
        thread_root_id,
        active_channel_id,
        active_thread_root_id,
    )
    .await
}

async fn schedule_queued_work_items(
    pool: &PgPool,
    registry: &WarmCodexRegistry,
) -> CommandResult<()> {
    let rows = sqlx::query(
        r#"
        select w.id, w.agent_id, w.channel_id, w.thread_root_id
        from agent_work_items w
        where w.status = 'queued'
          and not exists (
              select 1
              from supervisor_commands c
              where c.command_type = 'start_agent'
                and c.work_item_id = w.id
                and c.status in ('pending', 'running')
          )
        order by w.created_at asc
        limit 16
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    for row in rows {
        let work_item_id: Uuid = row.get("id");
        let agent_id: Uuid = row.get("agent_id");
        let channel_id: Option<Uuid> = row.get("channel_id");
        let thread_root_id: Option<Uuid> = row.get("thread_root_id");
        if !should_schedule_queued_work_item(pool, registry, agent_id, channel_id, thread_root_id)
            .await?
        {
            continue;
        }
        if enqueue_agent_work_if_available(pool, agent_id, work_item_id).await? {
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "dispatch",
                "Backlog agent request scheduled",
                work_item_id.to_string(),
            )
            .await?;
        }
    }

    Ok(())
}

async fn mark_orphaned_agent_runs(pool: &PgPool) -> CommandResult<()> {
    sqlx::query(
        r#"
        update agent_runs
        set status = 'unknown', stopped_at = coalesce(stopped_at, now())
        where stopped_at is null and status in ('starting', 'running', 'stopping')
        "#,
    )
    .execute(pool)
    .await
    .map_err(to_string)?;
    sqlx::query(
        "update agents set status = 'idle' where status in ('queued', 'running', 'stopping')",
    )
    .execute(pool)
    .await
    .map_err(to_string)?;
    sqlx::query(
        r#"
        update agent_work_items
        set status = 'failed',
            completed_at = now(),
            updated_at = now()
        where status = 'running'
        "#,
    )
    .execute(pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

async fn write_supervisor_heartbeat(pool: &PgPool) -> CommandResult<()> {
    sqlx::query(
        r#"
        insert into supervisor_state (id, pid, status, updated_at)
        values (1, $1, 'running', now())
        on conflict (id) do update set
            pid = excluded.pid,
            status = excluded.status,
            updated_at = excluded.updated_at
        "#,
    )
    .bind(std::process::id() as i32)
    .execute(pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

async fn claim_next_supervisor_command(pool: &PgPool) -> CommandResult<Option<SupervisorCommand>> {
    let row = sqlx::query(
        r#"
        update supervisor_commands
        set status = 'running',
            updated_at = now()
        where id = (
            select id
            from supervisor_commands
            where status = 'pending'
            order by created_at asc
            for update skip locked
            limit 1
        )
        returning id, command_type, agent_id, run_id, work_item_id
        "#,
    )
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;

    let Some(row) = row else {
        return Ok(None);
    };

    let command = SupervisorCommand {
        id: row.get("id"),
        command_type: row.get("command_type"),
        agent_id: row.get("agent_id"),
        run_id: row.get("run_id"),
        work_item_id: row.get("work_item_id"),
    };

    Ok(Some(command))
}

async fn finish_supervisor_command(
    pool: &PgPool,
    command_id: Uuid,
    error: Option<String>,
) -> CommandResult<()> {
    let (status, error) = match error {
        Some(error) => ("failed", error),
        None => ("done", String::new()),
    };

    sqlx::query(
        r#"
        update supervisor_commands
        set status = $2, error = $3, updated_at = now()
        where id = $1
        "#,
    )
    .bind(command_id)
    .bind(status)
    .bind(error)
    .execute(pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

async fn cleanup_supervisor_commands(pool: &PgPool) -> CommandResult<()> {
    sqlx::query(
        r#"
        delete from supervisor_commands
        where status in ('done', 'failed')
          and updated_at < now() - interval '7 days'
        "#,
    )
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

async fn process_supervisor_command(
    pool: &PgPool,
    codex_registry: &WarmCodexRegistry,
    claude_registry: &WarmClaudeRegistry,
    command: SupervisorCommand,
) -> CommandResult<()> {
    match command.command_type.as_str() {
        "start_agent" => {
            let Some(agent_id) = command.agent_id else {
                return Err("start_agent command missing agent_id".to_owned());
            };
            supervisor_start_agent(
                pool,
                codex_registry,
                claude_registry,
                agent_id,
                command.work_item_id,
            )
            .await
        }
        "stop_run" => {
            let Some(run_id) = command.run_id else {
                return Err("stop_run command missing run_id".to_owned());
            };
            supervisor_stop_run(pool, codex_registry, run_id).await
        }
        other => Err(format!("unknown supervisor command: {other}")),
    }
}

async fn interrupt_warm_codex_turn(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &Arc<WarmCodexRuntime>,
    run_id: Uuid,
) -> CommandResult<bool> {
    let (request_id, turn_id) = {
        let mut state = runtime.state.lock().await;
        let Some(active) = state.active.as_ref() else {
            return Ok(false);
        };
        if active.run_id != run_id {
            return Ok(false);
        }
        let Some(turn_id) = active.turn_id.clone() else {
            return Ok(false);
        };
        if active.interrupt_request_id.is_some() {
            return Ok(true);
        }
        let request_id = state.next_request_id;
        state.next_request_id += 1;
        state
            .active
            .as_mut()
            .expect("active turn checked")
            .interrupt_request_id = Some(request_id);
        state.last_activity = Instant::now();
        (request_id, turn_id)
    };

    {
        let mut stdin = runtime.stdin.lock().await;
        codex_write_json(
            &mut stdin,
            json!({
                "method": "turn/interrupt",
                "id": request_id,
                "params": {
                    "threadId": runtime.thread_id.clone(),
                    "turnId": turn_id
                }
            }),
        )
        .await?;
    }

    append_run_log(
        pool,
        run_id,
        format!("[codex] turn/interrupt requested id={request_id}\n"),
    )
    .await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Stop requested",
        turn_id,
    )
    .await?;
    Ok(true)
}

async fn steer_warm_codex_turn_if_same_surface(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &Arc<WarmCodexRuntime>,
    work_item_id: Uuid,
    codex_prompt: &str,
) -> CommandResult<bool> {
    let row = sqlx::query(
        r#"
        select channel_id, thread_root_id
        from agent_work_items
        where id = $1 and agent_id = $2 and status = 'queued'
        "#,
    )
    .bind(work_item_id)
    .bind(agent_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;
    let Some(row) = row else {
        return Ok(false);
    };
    let channel_id: Option<Uuid> = row.get("channel_id");
    let thread_root_id: Option<Uuid> = row.get("thread_root_id");

    let (active_run_id, active_channel_id, active_thread_root_id, active_turn_id) = {
        let state = runtime.state.lock().await;
        let Some(active) = state.active.as_ref() else {
            return Ok(false);
        };
        if active.steer_disabled {
            return Ok(false);
        }
        (
            active.run_id,
            active.channel_id,
            active.thread_root_id,
            active.turn_id.clone(),
        )
    };
    if !same_codex_surface(
        pool,
        channel_id,
        thread_root_id,
        active_channel_id,
        active_thread_root_id,
    )
    .await?
    {
        return Ok(false);
    }
    let Some(turn_id) = active_turn_id else {
        return Ok(false);
    };

    let request_id = {
        let mut state = runtime.state.lock().await;
        let Some(active) = state.active.as_mut() else {
            return Ok(false);
        };
        if active.steer_disabled {
            return Ok(false);
        }
        if active.run_id != active_run_id || active.turn_id.as_deref() != Some(turn_id.as_str()) {
            return Ok(false);
        }
        if active.channel_id != active_channel_id || active.thread_root_id != active_thread_root_id
        {
            return Ok(false);
        }
        let run_id = active.run_id;
        let request_id = state.next_request_id;
        state.next_request_id += 1;
        state.last_activity = Instant::now();
        state
            .active
            .as_mut()
            .expect("active turn checked")
            .steer_requests
            .insert(
                request_id,
                CodexSteerRequest {
                    work_item_id,
                    run_id,
                },
            );
        request_id
    };

    sqlx::query(
        r#"
        update agent_work_items
        set status = 'running',
            run_id = $2,
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(work_item_id)
    .bind(active_run_id)
    .execute(pool)
    .await
    .map_err(to_string)?;
    notify_ui_work_item_changed(pool, work_item_id, "work_item_running").await;

    let write_result = {
        let mut stdin = runtime.stdin.lock().await;
        codex_write_json(
            &mut stdin,
            json!({
                "method": "turn/steer",
                "id": request_id,
                "params": {
                    "threadId": runtime.thread_id.clone(),
                    "expectedTurnId": turn_id,
                    "input": [{
                        "type": "text",
                        "text": codex_prompt,
                        "text_elements": []
                    }]
                }
            }),
        )
        .await
    };

    if let Err(err) = write_result {
        {
            let mut state = runtime.state.lock().await;
            if let Some(active) = state.active.as_mut() {
                active.steer_requests.remove(&request_id);
            }
        }
        sqlx::query(
            r#"
            update agent_work_items
            set status = 'queued',
                run_id = null,
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_queued").await;
        return Err(err);
    }

    append_run_log(
        pool,
        active_run_id,
        format!("[codex] turn/steer work_item={work_item_id}\n"),
    )
    .await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(active_run_id),
        "dispatch",
        "Follow-up added",
        work_item_id.to_string(),
    )
    .await?;
    Ok(true)
}

async fn supervisor_start_codex_streaming_agent(
    pool: &PgPool,
    codex_registry: &WarmCodexRegistry,
    agent_id: Uuid,
    work_item_id: Option<Uuid>,
    handle: String,
    model: String,
    working_directory: String,
    work_item_prompt: String,
    memory_context: Option<String>,
) -> CommandResult<()> {
    let command_text = "codex app-server --listen stdio://".to_owned();
    let codex_prompt = build_codex_streaming_prompt(&work_item_prompt);
    let runtime = get_or_spawn_warm_codex_runtime(
        pool,
        codex_registry,
        agent_id,
        &handle,
        &model,
        &working_directory,
        memory_context.as_deref(),
    )
    .await?;

    {
        let state = runtime.state.lock().await;
        if !state.alive {
            return Err("codex warm runtime is not alive".to_owned());
        }
        if state.active.is_some() {
            drop(state);
            if let Some(work_item_id) = work_item_id {
                if steer_warm_codex_turn_if_same_surface(
                    pool,
                    agent_id,
                    &runtime,
                    work_item_id,
                    &codex_prompt,
                )
                .await?
                {
                    return Ok(());
                }
            }
            record_agent_activity_throttled(
                pool,
                Some(agent_id),
                None,
                "dispatch",
                "Agent busy",
                work_item_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "no agent request".to_owned()),
            )
            .await?;
            return Ok(());
        }
    }

    let initial_log = if codex_prompt.is_empty() {
        format!("$ {command_text}\n[warm process reused]\n")
    } else {
        format!(
            "$ {command_text}\n[warm process reused]\n\n[streaming agent request]\n{codex_prompt}\n"
        )
    };

    let run_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_runs (agent_id, work_item_id, command, working_directory, status, log)
        values ($1, $2, $3, $4, 'starting', $5)
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(work_item_id)
    .bind(&command_text)
    .bind(&working_directory)
    .bind(initial_log)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_created").await;

    let (channel_id, thread_root_id) = if let Some(work_item_id) = work_item_id {
        let row = sqlx::query(
            r#"
            select channel_id, thread_root_id
            from agent_work_items
            where id = $1 and agent_id = $2
            "#,
        )
        .bind(work_item_id)
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
        sqlx::query(
            r#"
            update agent_work_items
            set status = 'running',
                run_id = $2,
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_running").await;
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(run_id),
            "dispatch",
            "Request started",
            work_item_id.to_string(),
        )
        .await?;
        (
            row.get::<Option<Uuid>, _>("channel_id"),
            row.get::<Option<Uuid>, _>("thread_root_id"),
        )
    } else {
        (None, None)
    };

    sqlx::query("update agent_runs set status = 'running', pid = $2 where id = $1")
        .bind(run_id)
        .bind(runtime.pid)
        .execute(pool)
        .await
        .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_running").await;
    sqlx::query("update agents set status = 'running' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Started working",
        runtime
            .pid
            .map(|pid| format!("pid={pid}, thread_id={}", runtime.thread_id))
            .unwrap_or_else(|| format!("thread_id={}", runtime.thread_id)),
    )
    .await?;

    let request_id = {
        let mut state = runtime.state.lock().await;
        if !state.alive {
            return Err("codex warm runtime exited before turn start".to_owned());
        }
        if state.active.is_some() {
            return Err("codex warm runtime became busy before turn start".to_owned());
        }
        let request_id = state.next_request_id;
        state.next_request_id += 1;
        state.last_activity = Instant::now();
        state.active = Some(CodexActiveTurn {
            run_id,
            turn_request_id: request_id,
            turn_id: None,
            started_at: Instant::now(),
            first_delta_at: None,
            work_item_id,
            channel_id,
            thread_root_id,
            stream_keys: HashSet::new(),
            steer_requests: HashMap::new(),
            steer_disabled: false,
            interrupt_request_id: None,
        });
        request_id
    };

    let cwd = effective_codex_cwd(&working_directory)?;
    let model_value = codex_model_value(&model);
    let write_result = {
        let mut stdin = runtime.stdin.lock().await;
        codex_write_json(
            &mut stdin,
            json!({
                "method": "turn/start",
                "id": request_id,
                "params": {
                    "threadId": runtime.thread_id.clone(),
                    "input": [{
                        "type": "text",
                        "text": codex_prompt,
                        "text_elements": []
                    }],
                    "cwd": cwd,
                    "approvalPolicy": "never",
                    "model": model_value
                }
            }),
        )
        .await
    };

    if let Err(err) = write_result {
        finish_warm_codex_active_turn(pool, agent_id, &runtime, false, Some(err.clone())).await?;
        return Err(err);
    }

    Ok(())
}

async fn supervisor_start_claude_streaming_agent(
    pool: &PgPool,
    claude_registry: &WarmClaudeRegistry,
    agent_id: Uuid,
    work_item_id: Option<Uuid>,
    handle: String,
    model: String,
    working_directory: String,
    work_item_prompt: String,
    memory_context: Option<String>,
) -> CommandResult<()> {
    let claude_prompt = build_claude_streaming_prompt(&work_item_prompt);
    let model = if model.trim().is_empty() {
        "sonnet".to_owned()
    } else {
        model.trim().to_owned()
    };
    let command_text = claude_streaming_command_text(&model);
    let runtime = match get_or_spawn_warm_claude_runtime(
        pool,
        claude_registry,
        agent_id,
        &handle,
        &model,
        &working_directory,
        memory_context.as_deref(),
    )
    .await
    {
        Ok(runtime) => runtime,
        Err(err) => {
            if let Some(work_item_id) = work_item_id {
                sqlx::query(
                    r#"
                    update agent_work_items
                    set status = 'failed',
                        completed_at = now(),
                        updated_at = now()
                    where id = $1
                    "#,
                )
                .bind(work_item_id)
                .execute(pool)
                .await
                .map_err(to_string)?;
                notify_ui_work_item_changed(pool, work_item_id, "work_item_failed").await;
            }
            return Err(err);
        }
    };

    {
        let state = runtime.state.lock().await;
        if !state.alive {
            return Err("Claude warm runtime is not alive".to_owned());
        }
        if state.active.is_some() {
            drop(state);
            record_agent_activity_throttled(
                pool,
                Some(agent_id),
                None,
                "dispatch",
                "Agent busy",
                work_item_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "no agent request".to_owned()),
            )
            .await?;
            return Ok(());
        }
    }

    let initial_log = if claude_prompt.is_empty() {
        format!("$ {command_text}\n[warm process reused]\n")
    } else {
        format!(
            "$ {command_text}\n[warm process reused]\n\n[streaming agent request]\n{claude_prompt}\n"
        )
    };

    let run_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_runs (agent_id, work_item_id, command, working_directory, status, log)
        values ($1, $2, $3, $4, 'starting', $5)
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(work_item_id)
    .bind(&command_text)
    .bind(&working_directory)
    .bind(initial_log)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_created").await;

    let (channel_id, thread_root_id) = if let Some(work_item_id) = work_item_id {
        let row = sqlx::query(
            r#"
            select channel_id, thread_root_id
            from agent_work_items
            where id = $1 and agent_id = $2
            "#,
        )
        .bind(work_item_id)
        .bind(agent_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
        sqlx::query(
            r#"
            update agent_work_items
            set status = 'running',
                run_id = $2,
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_running").await;
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(run_id),
            "dispatch",
            "Request started",
            work_item_id.to_string(),
        )
        .await?;
        (
            row.get::<Option<Uuid>, _>("channel_id"),
            row.get::<Option<Uuid>, _>("thread_root_id"),
        )
    } else {
        (None, None)
    };

    sqlx::query("update agent_runs set status = 'running', pid = $2 where id = $1")
        .bind(run_id)
        .bind(runtime.pid)
        .execute(pool)
        .await
        .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_running").await;
    sqlx::query("update agents set status = 'running' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Started working",
        runtime
            .pid
            .map(|pid| format!("pid={pid}"))
            .unwrap_or_else(|| "pid unavailable".to_owned()),
    )
    .await?;

    let stream_key = claude_stream_key(run_id);
    {
        let mut state = runtime.state.lock().await;
        if !state.alive {
            return Err("Claude warm runtime exited before turn start".to_owned());
        }
        if state.active.is_some() {
            return Err("Claude warm runtime became busy before turn start".to_owned());
        }
        state.last_activity = Instant::now();
        state.active = Some(ClaudeActiveTurn {
            run_id,
            started_at: Instant::now(),
            first_delta_at: None,
            work_item_id,
            channel_id,
            thread_root_id,
            stream_key,
        });
    }

    let write_result = {
        let mut stdin = runtime.stdin.lock().await;
        claude_write_input(&mut stdin, claude_user_input(&claude_prompt)).await
    };

    if let Err(err) = write_result {
        finish_warm_claude_active_turn(pool, agent_id, &runtime, false, Some(err.clone())).await?;
        return Err(err);
    }

    Ok(())
}

async fn supervisor_start_agent(
    pool: &PgPool,
    codex_registry: &WarmCodexRegistry,
    claude_registry: &WarmClaudeRegistry,
    agent_id: Uuid,
    work_item_id: Option<Uuid>,
) -> CommandResult<()> {
    if let Some(work_item_id) = work_item_id {
        let status: Option<String> =
            sqlx::query_scalar("select status from agent_work_items where id = $1")
                .bind(work_item_id)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?;
        if status.as_deref() == Some("cancelled") {
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "dispatch",
                "Cancelled agent request skipped",
                work_item_id.to_string(),
            )
            .await?;
            return Ok(());
        }
    }

    if let Some(reason) = agent_budget_exhausted(pool, agent_id).await? {
        if let Some(work_item_id) = work_item_id {
            sqlx::query(
                r#"
                update agent_work_items
                set status = 'failed',
                    completed_at = now(),
                    updated_at = now()
                where id = $1
                "#,
            )
            .bind(work_item_id)
            .execute(pool)
            .await
            .map_err(to_string)?;
            notify_ui_work_item_changed(pool, work_item_id, "budget_exhausted").await;
        }
        record_agent_activity(
            pool,
            Some(agent_id),
            None,
            "usage",
            "Budget reached",
            reason,
        )
        .await?;
        return Ok(());
    }

    let row = sqlx::query(
        r#"
        select handle, runtime, model, launch_command, working_directory
        from agents
        where id = $1
        "#,
    )
    .bind(agent_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let handle: String = row.get("handle");
    let runtime: String = row.get("runtime");
    let model: String = row.get("model");
    let launch_command: String = row.get("launch_command");
    let working_directory: String = row.get::<String, _>("working_directory").trim().to_owned();
    let memory_context = match load_agent_memory_context(&working_directory) {
        Ok(memory_context) => memory_context,
        Err(err) => {
            record_agent_activity(
                pool,
                Some(agent_id),
                None,
                "profile",
                "Memory context skipped",
                err,
            )
            .await?;
            None
        }
    };
    let work_item_prompt = match work_item_id {
        Some(work_item_id) => {
            let row = sqlx::query(
                r#"
                select
                    w.channel_id,
                    w.title,
                    w.context,
                    c.name as channel_name,
                    t.number as task_number,
                    w.thread_root_id
                from agent_work_items w
                left join channels c on c.id = w.channel_id
                left join tasks t on t.id = w.task_id
                where w.id = $1 and w.agent_id = $2
                "#,
            )
            .bind(work_item_id)
            .bind(agent_id)
            .fetch_one(pool)
            .await
            .map_err(to_string)?;
            let title: String = row.get("title");
            let context: String = row.get("context");
            let channel_id: Option<Uuid> = row.get("channel_id");
            let channel_name: Option<String> = row.get("channel_name");
            let task_number: Option<i64> = row.get("task_number");
            let thread_root_id: Option<Uuid> = row.get("thread_root_id");
            let available_agents = load_channel_agent_roster(pool, channel_id, agent_id).await?;
            build_work_item_prompt(
                work_item_id,
                &title,
                &context,
                channel_name.as_deref(),
                task_number,
                thread_root_id,
                &available_agents,
            )
        }
        None => String::new(),
    };
    if runtime.eq_ignore_ascii_case("codex") {
        return supervisor_start_codex_streaming_agent(
            pool,
            codex_registry,
            agent_id,
            work_item_id,
            handle,
            model,
            working_directory,
            work_item_prompt,
            memory_context,
        )
        .await;
    }
    if runtime.eq_ignore_ascii_case("claude") {
        return supervisor_start_claude_streaming_agent(
            pool,
            claude_registry,
            agent_id,
            work_item_id,
            handle,
            model,
            working_directory,
            work_item_prompt,
            memory_context,
        )
        .await;
    }
    let work_item_prompt = prepend_memory_context(work_item_prompt, memory_context.as_deref());
    let command_text = effective_launch_command(launch_command, runtime, model, handle.clone());
    let initial_log = if work_item_prompt.is_empty() {
        format!("$ {command_text}\n")
    } else {
        format!("$ {command_text}\n\n[agent request]\n{work_item_prompt}\n")
    };

    let run_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_runs (agent_id, work_item_id, command, working_directory, status, log)
        values ($1, $2, $3, $4, 'starting', $5)
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(work_item_id)
    .bind(&command_text)
    .bind(&working_directory)
    .bind(initial_log)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_created").await;
    if let Some(work_item_id) = work_item_id {
        sqlx::query(
            r#"
            update agent_work_items
            set status = 'running',
                run_id = $2,
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_running").await;
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(run_id),
            "dispatch",
            "Request started",
            work_item_id.to_string(),
        )
        .await?;
    }
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Run created",
        "Supervisor is preparing the launch command",
    )
    .await?;

    let mut command = Command::new("/bin/zsh");
    command.arg("-lc").arg(&command_text);
    command.env("LOCAL_SLOCK_AGENT_ID", agent_id.to_string());
    command.env("LOCAL_SLOCK_AGENT_HANDLE", &handle);
    configure_agent_context_tool_env(&mut command);
    command.env("LOCAL_SLOCK_RUN_ID", run_id.to_string());
    command.env(
        "LOCAL_SLOCK_WORK_ITEM_ID",
        work_item_id
            .map(|id| id.to_string())
            .unwrap_or_else(String::new),
    );
    command.env("LOCAL_SLOCK_WORK_ITEM_PROMPT", &work_item_prompt);
    #[cfg(unix)]
    command.process_group(0);
    if !working_directory.is_empty() {
        command.current_dir(&working_directory);
    }
    command.stdout(Stdio::piped()).stderr(Stdio::piped());

    let mut child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            let error_log = format!("failed to start process: {err}\n");
            sqlx::query(
                r#"
                update agent_runs
                set status = 'failed', log = right(log || $2, 20000), stopped_at = now()
                where id = $1
                "#,
            )
            .bind(run_id)
            .bind(error_log)
            .execute(pool)
            .await
            .map_err(to_string)?;
            notify_ui_agent_run_changed(pool, run_id, "run_failed").await;
            sqlx::query("update agents set status = 'error' where id = $1")
                .bind(agent_id)
                .execute(pool)
                .await
                .map_err(to_string)?;
            if let Some(work_item_id) = work_item_id {
                sqlx::query(
                    r#"
                    update agent_work_items
                    set status = 'failed',
                        completed_at = now(),
                        updated_at = now()
                    where id = $1
                    "#,
                )
                .bind(work_item_id)
                .execute(pool)
                .await
                .map_err(to_string)?;
                notify_ui_work_item_changed(pool, work_item_id, "work_item_failed").await;
            }
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(run_id),
                "run_error",
                "Run failed to start",
                err.to_string(),
            )
            .await?;
            return Err(err.to_string());
        }
    };

    let pid = child.id().map(|id| id as i32);
    sqlx::query("update agent_runs set status = 'running', pid = $2 where id = $1")
        .bind(run_id)
        .bind(pid)
        .execute(pool)
        .await
        .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_running").await;
    sqlx::query("update agents set status = 'running' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Run started",
        pid.map(|pid| format!("pid={pid}"))
            .unwrap_or_else(|| "pid unavailable".to_owned()),
    )
    .await?;

    let mut pipe_tasks = Vec::new();
    if let Some(stdout) = child.stdout.take() {
        pipe_tasks.push(tokio::spawn(pipe_run_output(
            pool.clone(),
            agent_id,
            run_id,
            stdout,
            "stdout",
            true,
        )));
    }
    if let Some(stderr) = child.stderr.take() {
        pipe_tasks.push(tokio::spawn(pipe_run_output(
            pool.clone(),
            agent_id,
            run_id,
            stderr,
            "stderr",
            true,
        )));
    }

    tokio::spawn(wait_for_agent_run(
        pool.clone(),
        agent_id,
        run_id,
        work_item_id,
        pipe_tasks,
        child,
    ));

    Ok(())
}

async fn codex_warm_stdout_reader(
    pool: PgPool,
    registry: WarmCodexRegistry,
    agent_id: Uuid,
    runtime: Arc<WarmCodexRuntime>,
    reader: BufReader<tokio::process::ChildStdout>,
) {
    let mut lines = reader.lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Err(err) =
                    handle_codex_warm_stdout_line(&pool, agent_id, &runtime, &line).await
                {
                    let _ = record_agent_activity(
                        &pool,
                        Some(agent_id),
                        None,
                        "run_error",
                        "Codex stream event failed",
                        err,
                    )
                    .await;
                }
            }
            Ok(None) => break,
            Err(err) => {
                let _ = record_agent_activity(
                    &pool,
                    Some(agent_id),
                    None,
                    "run_error",
                    "Codex stdout read failed",
                    err.to_string(),
                )
                .await;
                break;
            }
        }
    }

    {
        let mut state = runtime.state.lock().await;
        state.alive = false;
    }
    let _ = finish_warm_codex_active_turn(
        &pool,
        agent_id,
        &runtime,
        false,
        Some("stdout closed".into()),
    )
    .await;
    remove_warm_codex_runtime_if_same(&registry, agent_id, &runtime).await;
}

async fn finish_codex_steer_request(
    pool: &PgPool,
    agent_id: Uuid,
    steer: CodexSteerRequest,
    success: bool,
    error: Option<String>,
) -> CommandResult<()> {
    let (status, completed_at, run_id) = if success {
        ("done", "now()", Some(steer.run_id))
    } else {
        ("queued", "null", None)
    };
    sqlx::query(&format!(
        r#"
        update agent_work_items
        set status = $2,
            run_id = $3,
            completed_at = {completed_at},
            updated_at = now()
        where id = $1
        "#
    ))
    .bind(steer.work_item_id)
    .bind(status)
    .bind(run_id)
    .execute(pool)
    .await
    .map_err(to_string)?;
    notify_ui_work_item_changed(pool, steer.work_item_id, "codex_turn_steer_result").await;

    record_agent_activity(
        pool,
        Some(agent_id),
        Some(steer.run_id),
        if success { "dispatch" } else { "run_error" },
        if success {
            "Follow-up added"
        } else {
            "Follow-up queued"
        },
        error.unwrap_or_else(|| steer.work_item_id.to_string()),
    )
    .await?;
    Ok(())
}

async fn claude_warm_stdout_reader<R>(
    pool: PgPool,
    registry: WarmClaudeRegistry,
    agent_id: Uuid,
    runtime: Arc<WarmClaudeRuntime>,
    stream: R,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                if let Err(err) =
                    handle_claude_warm_stdout_line(&pool, agent_id, &runtime, &line).await
                {
                    let _ = record_agent_activity(
                        &pool,
                        Some(agent_id),
                        None,
                        "run_error",
                        "Claude stream event failed",
                        err,
                    )
                    .await;
                }
            }
            Ok(None) => break,
            Err(err) => {
                let _ = record_agent_activity(
                    &pool,
                    Some(agent_id),
                    None,
                    "run_error",
                    "Claude stdout read failed",
                    err.to_string(),
                )
                .await;
                break;
            }
        }
    }

    {
        let mut state = runtime.state.lock().await;
        state.alive = false;
    }
    let _ = finish_warm_claude_active_turn(
        &pool,
        agent_id,
        &runtime,
        false,
        Some("stdout closed".to_owned()),
    )
    .await;
    remove_warm_claude_runtime_if_same(&registry, agent_id, &runtime).await;
}

async fn handle_claude_warm_stdout_line(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &Arc<WarmClaudeRuntime>,
    line: &str,
) -> CommandResult<()> {
    let active_run_id = {
        runtime
            .state
            .lock()
            .await
            .active
            .as_ref()
            .map(|active| active.run_id)
    };
    if let Some(run_id) = active_run_id {
        append_run_log(pool, run_id, format!("[claude] {line}\n")).await?;
    }
    let value: Value = serde_json::from_str(line).map_err(to_string)?;

    if let (Some(run_id), Some((input_tokens, output_tokens))) =
        (active_run_id, usage_from_runtime_event(&value))
    {
        let _ = record_run_usage(pool, agent_id, run_id, input_tokens, output_tokens, None).await;
    }

    if let Some(session_id) = claude_session_id(&value) {
        {
            let mut state = runtime.state.lock().await;
            state.session_id = Some(session_id.to_owned());
            state.last_activity = Instant::now();
        }
        let status = if active_run_id.is_some() {
            "running"
        } else {
            "idle"
        };
        let _ = upsert_runtime_thread_id(pool, agent_id, "claude", session_id, status).await;
    }

    if let Some((kind, title, detail)) = claude_stream_event_activity(&value) {
        if let Some(run_id) = active_run_id {
            record_agent_activity_throttled(
                pool,
                Some(agent_id),
                Some(run_id),
                kind,
                title,
                detail,
            )
            .await?;
        }
    }

    if let Some(delta) = claude_text_delta(&value) {
        let (active, first_delta_elapsed) = {
            let mut state = runtime.state.lock().await;
            let (active, first_delta_elapsed) = {
                let Some(active) = state.active.as_mut() else {
                    return Ok(());
                };
                let first_delta_elapsed = if active.first_delta_at.is_none() {
                    let elapsed = active.started_at.elapsed();
                    active.first_delta_at = Some(Instant::now());
                    Some(elapsed)
                } else {
                    None
                };
                let active = (
                    active.run_id,
                    active.channel_id,
                    active.thread_root_id,
                    active.stream_key.clone(),
                );
                (active, first_delta_elapsed)
            };
            state.last_activity = Instant::now();
            (active, first_delta_elapsed)
        };
        if let Some(elapsed) = first_delta_elapsed {
            record_agent_activity(
                pool,
                Some(agent_id),
                Some(active.0),
                "acting",
                "Responding",
                format!("first_token_ms={}", elapsed.as_millis()),
            )
            .await?;
        }
        if let Some(channel_id) = active.1 {
            append_streaming_agent_message(pool, agent_id, channel_id, active.2, &active.3, delta)
                .await?;
        }
        return Ok(());
    }

    if let Some(text) = claude_message_text(&value).or_else(|| claude_result_text(&value)) {
        let active = {
            let state = runtime.state.lock().await;
            state.active.as_ref().map(|active| {
                (
                    active.run_id,
                    active.channel_id,
                    active.thread_root_id,
                    active.stream_key.clone(),
                )
            })
        };
        if let Some((_, Some(channel_id), thread_root_id, stream_key)) = active {
            if !streaming_message_exists(pool, &stream_key).await? {
                append_streaming_agent_message(
                    pool,
                    agent_id,
                    channel_id,
                    thread_root_id,
                    &stream_key,
                    &text,
                )
                .await?;
            }
        }
    }

    if let Some(error) = claude_result_error(&value) {
        finish_warm_claude_active_turn(pool, agent_id, runtime, false, Some(error)).await?;
    } else if value.get("type").and_then(Value::as_str) == Some("result") {
        finish_warm_claude_active_turn(pool, agent_id, runtime, true, None).await?;
    }

    Ok(())
}

async fn claude_warm_stderr_reader<R>(
    pool: PgPool,
    agent_id: Uuid,
    runtime: Arc<WarmClaudeRuntime>,
    stream: R,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let active_run_id = {
            runtime
                .state
                .lock()
                .await
                .active
                .as_ref()
                .map(|active| active.run_id)
        };
        if let Some(run_id) = active_run_id {
            let _ = append_run_log(&pool, run_id, format!("[stderr] {line}\n")).await;
            if let Some((kind, title, detail)) = classify_agent_output_activity("stderr", &line) {
                let _ = record_agent_activity_throttled(
                    &pool,
                    Some(agent_id),
                    Some(run_id),
                    kind,
                    title,
                    detail,
                )
                .await;
            }
        }
    }
}

async fn handle_codex_warm_stdout_line(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &Arc<WarmCodexRuntime>,
    line: &str,
) -> CommandResult<()> {
    let value: Value = serde_json::from_str(line).map_err(to_string)?;
    let active_run_id = {
        runtime
            .state
            .lock()
            .await
            .active
            .as_ref()
            .map(|active| active.run_id)
    };
    if let Some(run_id) = active_run_id {
        append_run_log(pool, run_id, format!("[codex] {line}\n")).await?;
    }

    if let (Some(run_id), Some((input_tokens, output_tokens))) =
        (active_run_id, usage_from_runtime_event(&value))
    {
        let _ = record_run_usage(pool, agent_id, run_id, input_tokens, output_tokens, None).await;
    }

    if let Some(response_id) = value.get("id").and_then(Value::as_i64) {
        let matched = {
            let mut state = runtime.state.lock().await;
            let Some(active) = state.active.as_mut() else {
                return Ok(());
            };
            if active.turn_request_id == response_id {
                if let Some(turn_id) = codex_turn_id_from_value(&value) {
                    active.turn_id = Some(turn_id);
                    state.last_activity = Instant::now();
                }
                Some((true, None, false))
            } else if let Some(steer) = active.steer_requests.remove(&response_id) {
                if codex_request_error(&value).is_some() {
                    active.steer_disabled = true;
                }
                state.last_activity = Instant::now();
                Some((false, Some(steer), false))
            } else if active.interrupt_request_id == Some(response_id) {
                active.interrupt_request_id = None;
                state.last_activity = Instant::now();
                Some((false, None, true))
            } else {
                None
            }
        };
        if let Some((is_turn_start, steer, is_interrupt)) = matched {
            if is_turn_start {
                if let Some(error) = codex_request_error(&value) {
                    finish_warm_codex_active_turn(pool, agent_id, runtime, false, Some(error))
                        .await?;
                } else if let Some(run_id) = active_run_id {
                    record_agent_activity(
                        pool,
                        Some(agent_id),
                        Some(run_id),
                        "run",
                        "Request acknowledged",
                        response_id.to_string(),
                    )
                    .await?;
                }
                return Ok(());
            }
            if is_interrupt {
                if let Some(error) = codex_request_error(&value) {
                    finish_warm_codex_active_turn(pool, agent_id, runtime, false, Some(error))
                        .await?;
                } else if let Some(run_id) = active_run_id {
                    record_agent_activity(
                        pool,
                        Some(agent_id),
                        Some(run_id),
                        "run",
                        "Stop acknowledged",
                        response_id.to_string(),
                    )
                    .await?;
                }
                return Ok(());
            }
            if let Some(steer) = steer {
                if let Some(error) = codex_request_error(&value) {
                    finish_codex_steer_request(pool, agent_id, steer, false, Some(error)).await?;
                } else {
                    finish_codex_steer_request(pool, agent_id, steer, true, None).await?;
                }
                return Ok(());
            }
        }
    }

    match value.get("method").and_then(Value::as_str) {
        Some("turn/started") => {
            if value.pointer("/params/threadId").and_then(Value::as_str)
                != Some(runtime.thread_id.as_str())
            {
                return Ok(());
            }
            if let Some(turn_id) = codex_turn_id_from_value(&value) {
                let mut state = runtime.state.lock().await;
                if let Some(active) = state.active.as_mut() {
                    active.turn_id = Some(turn_id);
                    state.last_activity = Instant::now();
                }
            }
        }
        Some("item/agentMessage/delta") => {
            let item_id = value
                .pointer("/params/itemId")
                .and_then(Value::as_str)
                .unwrap_or("agent-message");
            let delta = value
                .pointer("/params/delta")
                .and_then(Value::as_str)
                .unwrap_or("");
            if delta.is_empty() {
                return Ok(());
            }
            let (active, first_delta_elapsed) = {
                let mut state = runtime.state.lock().await;
                let Some(active) = state.active.as_mut() else {
                    return Ok(());
                };
                let first_delta_elapsed = if active.first_delta_at.is_none() {
                    let elapsed = active.started_at.elapsed();
                    active.first_delta_at = Some(Instant::now());
                    Some(elapsed)
                } else {
                    None
                };
                let stream_key = codex_stream_key(active.run_id, item_id);
                active.stream_keys.insert(stream_key.clone());
                let active = (
                    active.run_id,
                    active.channel_id,
                    active.thread_root_id,
                    stream_key,
                );
                (active, first_delta_elapsed)
            };
            if let Some(elapsed) = first_delta_elapsed {
                record_agent_activity(
                    pool,
                    Some(agent_id),
                    Some(active.0),
                    "acting",
                    "Responding",
                    format!("first_token_ms={}", elapsed.as_millis()),
                )
                .await?;
            }
            if let Some(channel_id) = active.1 {
                append_streaming_agent_message(
                    pool, agent_id, channel_id, active.2, &active.3, delta,
                )
                .await?;
            }
        }
        Some("item/completed") if codex_item_type(&value) == Some("agentMessage") => {
            let Some(item_id) = codex_item_id(&value) else {
                return Ok(());
            };
            let active = {
                let mut state = runtime.state.lock().await;
                let Some(active) = state.active.as_mut() else {
                    return Ok(());
                };
                let stream_key = codex_stream_key(active.run_id, item_id);
                active.stream_keys.remove(&stream_key);
                (
                    active.run_id,
                    active.channel_id,
                    active.thread_root_id,
                    active.work_item_id,
                    stream_key,
                )
            };
            if let Some(channel_id) = active.1 {
                let existing: Option<Uuid> =
                    sqlx::query_scalar("select id from messages where stream_key = $1")
                        .bind(&active.4)
                        .fetch_optional(pool)
                        .await
                        .map_err(to_string)?;
                if existing.is_none() {
                    if let Some(text) = value
                        .pointer("/params/item/text")
                        .and_then(Value::as_str)
                        .filter(|text| !text.is_empty())
                    {
                        append_streaming_agent_message(
                            pool, agent_id, channel_id, active.2, &active.4, text,
                        )
                        .await?;
                    }
                }
                let hidden = consume_streaming_agent_control_lines(
                    pool, agent_id, active.0, active.3, &active.4,
                )
                .await?;
                if !hidden {
                    finish_streaming_agent_message(pool, &active.4, "complete").await?;
                }
            }
        }
        Some("item/completed") => {
            let Some(run_id) = active_run_id else {
                return Ok(());
            };
            if let Some((kind, title, detail)) = codex_tool_completion_activity(&value) {
                record_agent_activity_throttled(
                    pool,
                    Some(agent_id),
                    Some(run_id),
                    kind,
                    title,
                    detail,
                )
                .await?;
            }
        }
        Some("item/started") => {
            let Some(run_id) = active_run_id else {
                return Ok(());
            };
            let (kind, title, detail) = codex_item_started_activity(&value);
            record_agent_activity_throttled(
                pool,
                Some(agent_id),
                Some(run_id),
                kind,
                title,
                detail,
            )
            .await?;
        }
        Some("item/reasoning/textDelta") | Some("item/reasoning/summaryTextDelta") => {
            let Some(run_id) = active_run_id else {
                return Ok(());
            };
            if let Some(delta) = value.pointer("/params/delta").and_then(Value::as_str) {
                if !delta.trim().is_empty() {
                    append_run_log(pool, run_id, format!("[thinking] {delta}\n")).await?;
                }
            }
        }
        Some("turn/completed") => {
            if value.pointer("/params/threadId").and_then(Value::as_str)
                == Some(runtime.thread_id.as_str())
            {
                finish_warm_codex_active_turn(pool, agent_id, runtime, true, None).await?;
            }
        }
        Some("error") => {
            let detail = value
                .pointer("/params/message")
                .and_then(Value::as_str)
                .unwrap_or("codex emitted error notification")
                .to_owned();
            finish_warm_codex_active_turn(pool, agent_id, runtime, false, Some(detail)).await?;
        }
        _ => {}
    }

    Ok(())
}

async fn codex_warm_stderr_reader<R>(
    pool: PgPool,
    agent_id: Uuid,
    runtime: Arc<WarmCodexRuntime>,
    stream: R,
) where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    while let Ok(Some(line)) = lines.next_line().await {
        let active_run_id = {
            runtime
                .state
                .lock()
                .await
                .active
                .as_ref()
                .map(|active| active.run_id)
        };
        if let Some(run_id) = active_run_id {
            let _ = append_run_log(&pool, run_id, format!("[stderr] {line}\n")).await;
            if let Some((kind, title, detail)) = classify_agent_output_activity("stderr", &line) {
                let _ = record_agent_activity_throttled(
                    &pool,
                    Some(agent_id),
                    Some(run_id),
                    kind,
                    title,
                    detail,
                )
                .await;
            }
        }
    }
}

async fn wait_for_warm_codex_process(
    pool: PgPool,
    registry: WarmCodexRegistry,
    agent_id: Uuid,
    runtime: Arc<WarmCodexRuntime>,
    mut child: tokio::process::Child,
) {
    let wait_result = child.wait().await;
    {
        let mut state = runtime.state.lock().await;
        state.alive = false;
    }
    let detail = match wait_result {
        Ok(status) => format!("codex app-server exited: {status}"),
        Err(err) => format!("codex app-server wait failed: {err}"),
    };
    let _ =
        finish_warm_codex_active_turn(&pool, agent_id, &runtime, false, Some(detail.clone())).await;
    let _ = sqlx::query(
        "update runtime_sessions set status = 'stopped', updated_at = now() where agent_id = $1 and runtime = 'codex'",
    )
    .bind(agent_id)
    .execute(&pool)
    .await;
    let _ = record_agent_activity(
        &pool,
        Some(agent_id),
        None,
        "run",
        "Codex warm app-server exited",
        detail,
    )
    .await;
    remove_warm_codex_runtime_if_same(&registry, agent_id, &runtime).await;
    let _ = notify_supervisor_wake(&pool).await;
}

async fn remove_warm_codex_runtime_if_same(
    registry: &WarmCodexRegistry,
    agent_id: Uuid,
    runtime: &Arc<WarmCodexRuntime>,
) {
    let mut runtimes = registry.runtimes.lock().await;
    if runtimes
        .get(&agent_id)
        .map(|current| Arc::ptr_eq(current, runtime))
        .unwrap_or(false)
    {
        runtimes.remove(&agent_id);
    }
}

async fn terminate_process_group(pid: i32) -> CommandResult<()> {
    let status = Command::new("kill")
        .arg("-TERM")
        .arg(format!("-{pid}"))
        .status()
        .await
        .map_err(to_string)?;

    if !status.success() {
        return Err(format!("failed to terminate process group {pid}: {status}"));
    }

    Ok(())
}

async fn codex_warm_idle_reaper(pool: PgPool, agent_id: Uuid, runtime: Arc<WarmCodexRuntime>) {
    loop {
        sleep(CODEX_IDLE_REAPER_INTERVAL).await;
        let should_stop = {
            let mut state = runtime.state.lock().await;
            let should_stop = state.alive
                && state.active.is_none()
                && state.last_activity.elapsed() >= CODEX_IDLE_TIMEOUT;
            if should_stop {
                state.alive = false;
            }
            should_stop
        };
        if !should_stop {
            if !runtime.state.lock().await.alive {
                return;
            }
            continue;
        }

        let Some(pid) = runtime.pid else {
            return;
        };
        match terminate_process_group(pid).await {
            Ok(()) => {
                let _ = sqlx::query(
                    "update runtime_sessions set status = 'stopping', updated_at = now() where agent_id = $1 and runtime = 'codex'",
                )
                .bind(agent_id)
                .execute(&pool)
                .await;
                let _ = record_agent_activity(
                    &pool,
                    Some(agent_id),
                    None,
                    "run",
                    "Codex warm app-server idle timeout",
                    format!("sent SIGTERM to process_group={pid}"),
                )
                .await;
            }
            Err(err) => {
                {
                    let mut state = runtime.state.lock().await;
                    state.alive = true;
                    state.last_activity = Instant::now();
                }
                let _ = record_agent_activity(
                    &pool,
                    Some(agent_id),
                    None,
                    "run_error",
                    "Codex warm app-server idle stop failed",
                    err,
                )
                .await;
            }
        }
        return;
    }
}

async fn wait_for_warm_claude_process(
    pool: PgPool,
    registry: WarmClaudeRegistry,
    agent_id: Uuid,
    runtime: Arc<WarmClaudeRuntime>,
    mut child: tokio::process::Child,
) {
    let wait_result = child.wait().await;
    {
        let mut state = runtime.state.lock().await;
        state.alive = false;
    }
    let detail = match wait_result {
        Ok(status) => format!("claude stream-json exited: {status}"),
        Err(err) => format!("claude stream-json wait failed: {err}"),
    };
    let _ = finish_warm_claude_active_turn(&pool, agent_id, &runtime, false, Some(detail.clone()))
        .await;
    let _ = sqlx::query(
        "update runtime_sessions set status = 'stopped', updated_at = now() where agent_id = $1 and runtime = 'claude'",
    )
    .bind(agent_id)
    .execute(&pool)
    .await;
    let _ = record_agent_activity(
        &pool,
        Some(agent_id),
        None,
        "run",
        "Claude warm stream-json exited",
        detail,
    )
    .await;
    remove_warm_claude_runtime_if_same(&registry, agent_id, &runtime).await;
    let _ = notify_supervisor_wake(&pool).await;
}

async fn remove_warm_claude_runtime_if_same(
    registry: &WarmClaudeRegistry,
    agent_id: Uuid,
    runtime: &Arc<WarmClaudeRuntime>,
) {
    let mut runtimes = registry.runtimes.lock().await;
    if runtimes
        .get(&agent_id)
        .map(|current| Arc::ptr_eq(current, runtime))
        .unwrap_or(false)
    {
        runtimes.remove(&agent_id);
    }
}

async fn claude_warm_idle_reaper(pool: PgPool, agent_id: Uuid, runtime: Arc<WarmClaudeRuntime>) {
    loop {
        sleep(CODEX_IDLE_REAPER_INTERVAL).await;
        let should_stop = {
            let mut state = runtime.state.lock().await;
            let should_stop = state.alive
                && state.active.is_none()
                && state.last_activity.elapsed() >= CODEX_IDLE_TIMEOUT;
            if should_stop {
                state.alive = false;
            }
            should_stop
        };
        if !should_stop {
            if !runtime.state.lock().await.alive {
                return;
            }
            continue;
        }

        let Some(pid) = runtime.pid else {
            return;
        };
        match terminate_process_group(pid).await {
            Ok(()) => {
                let _ = sqlx::query(
                    "update runtime_sessions set status = 'stopping', updated_at = now() where agent_id = $1 and runtime = 'claude'",
                )
                .bind(agent_id)
                .execute(&pool)
                .await;
                let _ = record_agent_activity(
                    &pool,
                    Some(agent_id),
                    None,
                    "run",
                    "Claude warm stream-json idle timeout",
                    format!("sent SIGTERM to process_group={pid}"),
                )
                .await;
            }
            Err(err) => {
                {
                    let mut state = runtime.state.lock().await;
                    state.alive = true;
                    state.last_activity = Instant::now();
                }
                let _ = record_agent_activity(
                    &pool,
                    Some(agent_id),
                    None,
                    "run_error",
                    "Claude warm stream-json idle stop failed",
                    err,
                )
                .await;
            }
        }
        return;
    }
}

async fn finish_warm_claude_active_turn(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &Arc<WarmClaudeRuntime>,
    success: bool,
    error: Option<String>,
) -> CommandResult<()> {
    let (active, session_id) = {
        let mut state = runtime.state.lock().await;
        state.last_activity = Instant::now();
        let active = state.active.take();
        let session_id = state.session_id.clone();
        (active, session_id)
    };
    let Some(active) = active else {
        return Ok(());
    };
    let elapsed_ms = active.started_at.elapsed().as_millis();
    let current_work_status: Option<String> = match active.work_item_id {
        Some(work_item_id) => {
            sqlx::query_scalar("select status from agent_work_items where id = $1")
                .bind(work_item_id)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?
        }
        None => None,
    };
    let was_cancelled = current_work_status.as_deref() == Some("cancelling");

    let hidden = if success && !was_cancelled {
        consume_streaming_agent_control_lines(
            pool,
            agent_id,
            active.run_id,
            active.work_item_id,
            &active.stream_key,
        )
        .await?
    } else {
        false
    };
    if !hidden {
        finish_streaming_agent_message(
            pool,
            &active.stream_key,
            if success && !was_cancelled {
                "complete"
            } else {
                "error"
            },
        )
        .await?;
    }

    let run_status = if was_cancelled {
        "cancelled"
    } else if success {
        "exited"
    } else {
        "failed"
    };
    let agent_status = if success || was_cancelled {
        "idle"
    } else {
        "error"
    };
    let log_line = if was_cancelled {
        "claude warm turn cancelled\n".to_owned()
    } else {
        error
            .as_ref()
            .map(|error| format!("claude warm turn failed: {error}\n"))
            .unwrap_or_else(|| format!("claude warm turn completed in {elapsed_ms} ms\n"))
    };
    sqlx::query(
        r#"
        update agent_runs
        set status = $2,
            exit_code = null,
            log = right(log || $3, 20000),
            stopped_at = now()
        where id = $1
        "#,
    )
    .bind(active.run_id)
    .bind(run_status)
    .bind(&log_line)
    .execute(pool)
    .await
    .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, active.run_id, "claude_turn_finished").await;

    sqlx::query("update agents set status = $2 where id = $1")
        .bind(agent_id)
        .bind(agent_status)
        .execute(pool)
        .await
        .map_err(to_string)?;

    if let Some(work_item_id) = active.work_item_id {
        let current_work_status: Option<String> =
            sqlx::query_scalar("select status from agent_work_items where id = $1")
                .bind(work_item_id)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?;
        let was_silent = current_work_status.as_deref() == Some("silent");
        let work_status = if was_cancelled {
            "cancelled"
        } else if was_silent && success {
            "silent"
        } else if success {
            "done"
        } else {
            "failed"
        };
        sqlx::query(
            r#"
            update agent_work_items
            set status = $2,
                completed_at = now(),
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .bind(work_status)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_finished").await;
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(active.run_id),
            "dispatch",
            work_status_title(work_status),
            work_item_id.to_string(),
        )
        .await?;
    }

    let provider_thread_id = session_id.unwrap_or_else(|| {
        runtime
            .pid
            .map(|pid| format!("pid:{pid}"))
            .unwrap_or_else(|| "unknown".to_owned())
    });
    upsert_runtime_thread_id(
        pool,
        agent_id,
        "claude",
        &provider_thread_id,
        if success || was_cancelled {
            "idle"
        } else {
            "failed"
        },
    )
    .await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(active.run_id),
        if success || was_cancelled {
            "run"
        } else {
            "run_error"
        },
        if was_cancelled {
            "Stopped"
        } else if success {
            "Completed"
        } else {
            "Failed"
        },
        if success || was_cancelled {
            format!("duration={elapsed_ms} ms")
        } else {
            log_line.trim().to_owned()
        },
    )
    .await?;
    let _ = notify_supervisor_wake(pool).await;
    Ok(())
}

async fn finish_warm_codex_active_turn(
    pool: &PgPool,
    agent_id: Uuid,
    runtime: &Arc<WarmCodexRuntime>,
    success: bool,
    error: Option<String>,
) -> CommandResult<()> {
    let active = {
        let mut state = runtime.state.lock().await;
        state.last_activity = Instant::now();
        state.active.take()
    };
    let Some(active) = active else {
        return Ok(());
    };
    let elapsed_ms = active.started_at.elapsed().as_millis();
    let current_work_status: Option<String> = match active.work_item_id {
        Some(work_item_id) => {
            sqlx::query_scalar("select status from agent_work_items where id = $1")
                .bind(work_item_id)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?
        }
        None => None,
    };
    let was_cancelled = current_work_status.as_deref() == Some("cancelling");

    for stream_key in active.stream_keys {
        let hidden = if success && !was_cancelled {
            consume_streaming_agent_control_lines(
                pool,
                agent_id,
                active.run_id,
                active.work_item_id,
                &stream_key,
            )
            .await?
        } else {
            false
        };
        if !hidden {
            finish_streaming_agent_message(
                pool,
                &stream_key,
                if success && !was_cancelled {
                    "complete"
                } else {
                    "error"
                },
            )
            .await?;
        }
    }

    for steer in active.steer_requests.into_values() {
        finish_codex_steer_request(
            pool,
            agent_id,
            steer,
            success && !was_cancelled,
            if was_cancelled {
                Some("cancelled".to_owned())
            } else {
                error.clone()
            },
        )
        .await?;
    }

    let run_status = if was_cancelled {
        "cancelled"
    } else if success {
        "exited"
    } else {
        "failed"
    };
    let agent_status = if success || was_cancelled {
        "idle"
    } else {
        "error"
    };
    let log_line = if was_cancelled {
        "codex warm turn cancelled\n".to_owned()
    } else {
        error
            .as_ref()
            .map(|error| format!("codex warm turn failed: {error}\n"))
            .unwrap_or_else(|| format!("codex warm turn completed in {elapsed_ms} ms\n"))
    };
    sqlx::query(
        r#"
        update agent_runs
        set status = $2,
            exit_code = null,
            log = right(log || $3, 20000),
            stopped_at = now()
        where id = $1
        "#,
    )
    .bind(active.run_id)
    .bind(run_status)
    .bind(&log_line)
    .execute(pool)
    .await
    .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, active.run_id, "codex_turn_finished").await;

    sqlx::query("update agents set status = $2 where id = $1")
        .bind(agent_id)
        .bind(agent_status)
        .execute(pool)
        .await
        .map_err(to_string)?;

    if let Some(work_item_id) = active.work_item_id {
        let current_work_status: Option<String> =
            sqlx::query_scalar("select status from agent_work_items where id = $1")
                .bind(work_item_id)
                .fetch_optional(pool)
                .await
                .map_err(to_string)?;
        let was_silent = current_work_status.as_deref() == Some("silent");
        let work_status = if was_cancelled {
            "cancelled"
        } else if was_silent && success {
            "silent"
        } else if success {
            "done"
        } else {
            "failed"
        };
        sqlx::query(
            r#"
            update agent_work_items
            set status = $2,
                completed_at = now(),
                updated_at = now()
            where id = $1
            "#,
        )
        .bind(work_item_id)
        .bind(work_status)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_finished").await;
        record_agent_activity(
            pool,
            Some(agent_id),
            Some(active.run_id),
            "dispatch",
            work_status_title(work_status),
            work_item_id.to_string(),
        )
        .await?;
    }

    upsert_runtime_thread_id(
        pool,
        agent_id,
        "codex",
        &runtime.thread_id,
        if success || was_cancelled {
            "idle"
        } else {
            "failed"
        },
    )
    .await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(active.run_id),
        if success || was_cancelled {
            "run"
        } else {
            "run_error"
        },
        if was_cancelled {
            "Stopped"
        } else if success {
            "Completed"
        } else {
            "Failed"
        },
        if success || was_cancelled {
            format!("duration={elapsed_ms} ms")
        } else {
            log_line.trim().to_owned()
        },
    )
    .await?;
    let _ = notify_supervisor_wake(pool).await;
    Ok(())
}

async fn supervisor_stop_run(
    pool: &PgPool,
    codex_registry: &WarmCodexRegistry,
    run_id: Uuid,
) -> CommandResult<()> {
    let row = sqlx::query(
        r#"
        select r.agent_id, r.pid, r.work_item_id, a.runtime
        from agent_runs r
        join agents a on a.id = r.agent_id
        where r.id = $1 and r.stopped_at is null
        "#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let agent_id: Uuid = row.get("agent_id");
    let pid: Option<i32> = row.get("pid");
    let work_item_id: Option<Uuid> = row.get("work_item_id");
    let runtime: String = row.get("runtime");
    let Some(pid) = pid else {
        return Err("agent run does not have a pid yet".to_owned());
    };

    sqlx::query("update agent_runs set status = 'stopping' where id = $1")
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    notify_ui_agent_run_changed(pool, run_id, "run_stopping").await;
    sqlx::query("update agents set status = 'stopping' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    if let Some(work_item_id) = work_item_id {
        sqlx::query(
            r#"
            update agent_work_items
            set status = 'cancelling',
                updated_at = now()
            where id = $1 and status in ('queued', 'running')
            "#,
        )
        .bind(work_item_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
        notify_ui_work_item_changed(pool, work_item_id, "work_item_cancelling").await;
    }

    if runtime.eq_ignore_ascii_case("codex") {
        let warm_runtime = {
            let runtimes = codex_registry.runtimes.lock().await;
            runtimes.get(&agent_id).cloned()
        };
        if let Some(runtime) = warm_runtime {
            if interrupt_warm_codex_turn(pool, agent_id, &runtime, run_id).await? {
                record_agent_activity(
                    pool,
                    Some(agent_id),
                    Some(run_id),
                    "run",
                    "Stop requested",
                    "Codex accepted stop request",
                )
                .await?;
                return Ok(());
            }
        }
    }

    terminate_process_group(pid).await?;

    append_run_log(
        pool,
        run_id,
        format!("sent SIGTERM to process group {pid}\n"),
    )
    .await?;
    record_agent_activity(
        pool,
        Some(agent_id),
        Some(run_id),
        "run",
        "Stop requested",
        format!("process_group={pid}"),
    )
    .await?;
    Ok(())
}

fn spawn_supervisor_process(database_url: &str) {
    let Ok(exe) = env::current_exe() else {
        eprintln!("failed to resolve current executable for LocalSlock supervisor");
        return;
    };

    if let Err(err) = StdCommand::new(exe)
        .arg("--supervisor")
        .env("LOCAL_SLOCK_DATABASE_URL", database_url)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        eprintln!("failed to spawn LocalSlock supervisor: {err}");
    }
}

fn launch_agent_plist_path() -> CommandResult<PathBuf> {
    let home = env::var_os("HOME").ok_or_else(|| "HOME is not set".to_owned())?;
    Ok(PathBuf::from(home)
        .join("Library")
        .join("LaunchAgents")
        .join(format!("{LAUNCH_AGENT_LABEL}.plist")))
}

fn launch_agent_domain() -> CommandResult<String> {
    let output = StdCommand::new("id")
        .arg("-u")
        .output()
        .map_err(to_string)?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_owned());
    }
    let uid = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if uid.is_empty() {
        return Err("failed to resolve current uid".to_owned());
    }
    Ok(format!("gui/{uid}"))
}

fn launch_agent_service_target(domain: &str) -> String {
    format!("{domain}/{LAUNCH_AGENT_LABEL}")
}

fn run_launchctl(args: &[&str]) -> CommandResult<()> {
    let output = StdCommand::new("launchctl")
        .args(args)
        .output()
        .map_err(to_string)?;
    if output.status.success() {
        return Ok(());
    }

    let stderr = String::from_utf8_lossy(&output.stderr);
    let stdout = String::from_utf8_lossy(&output.stdout);
    Err(format!(
        "launchctl {} failed: {}{}",
        args.join(" "),
        stderr.trim(),
        if stdout.trim().is_empty() {
            String::new()
        } else {
            format!(" {}", stdout.trim())
        }
    ))
}

fn render_launch_agent_plist(exe_path: &std::path::Path, database_url: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{}</string>
    <string>--supervisor</string>
  </array>
  <key>EnvironmentVariables</key>
  <dict>
    <key>LOCAL_SLOCK_DATABASE_URL</key>
    <string>{}</string>
  </dict>
  <key>RunAtLoad</key>
  <true/>
  <key>KeepAlive</key>
  <true/>
  <key>StandardOutPath</key>
  <string>{}</string>
  <key>StandardErrorPath</key>
  <string>{}</string>
</dict>
</plist>
"#,
        xml_escape(LAUNCH_AGENT_LABEL),
        xml_escape(&exe_path.to_string_lossy()),
        xml_escape(database_url),
        xml_escape("/tmp/localslock-supervisor.out.log"),
        xml_escape("/tmp/localslock-supervisor.err.log"),
    )
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn normalize_channel_name(name: &str) -> String {
    name.trim()
        .trim_start_matches('#')
        .to_lowercase()
        .replace(' ', "-")
}

fn to_string(error: impl std::fmt::Display) -> String {
    error.to_string()
}

struct AgentContextTarget {
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    label: String,
}

fn arg_value(args: &[String], name: &str) -> Option<String> {
    args.windows(2)
        .find_map(|window| (window[0] == name).then(|| window[1].clone()))
}

fn has_arg(args: &[String], name: &str) -> bool {
    args.iter().any(|arg| arg == name)
}

fn parse_context_tool_limit(args: &[String], default: i64, max: i64) -> CommandResult<i64> {
    let Some(raw) = arg_value(args, "--limit") else {
        return Ok(default);
    };
    let parsed = raw
        .parse::<i64>()
        .map_err(|_| format!("invalid --limit value: {raw}"))?;
    Ok(parsed.clamp(1, max))
}

fn short_id(id: Uuid) -> String {
    id.to_string().chars().take(8).collect()
}

fn split_context_target(raw_target: &str) -> (String, Option<String>) {
    let target = raw_target.trim();
    if let Some(rest) = target.strip_prefix("dm:@") {
        if let Some((handle, thread)) = rest.split_once(':') {
            return (format!("dm:@{handle}"), Some(thread.to_owned()));
        }
        return (target.to_owned(), None);
    }
    if let Some(rest) = target.strip_prefix('#') {
        if let Some((channel, thread)) = rest.split_once(':') {
            return (format!("#{channel}"), Some(thread.to_owned()));
        }
    }
    if let Some((channel, thread)) = target.split_once(':') {
        return (channel.to_owned(), Some(thread.to_owned()));
    }
    (target.to_owned(), None)
}

async fn resolve_agent_context_channel(
    pool: &PgPool,
    channel_ref: &str,
) -> CommandResult<(Uuid, String)> {
    let channel_ref = channel_ref.trim();
    if channel_ref.is_empty() {
        return Err("target channel is empty".to_owned());
    }

    if let Some(handle) = channel_ref.strip_prefix("dm:@") {
        let row = sqlx::query(
            r#"
            select c.id, a.handle
            from channels c
            join agents a on a.id = c.dm_agent_id
            where c.kind = 'dm' and lower(a.handle) = lower($1)
            "#,
        )
        .bind(handle)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?;
        let Some(row) = row else {
            return Err(format!("unknown DM target: {channel_ref}"));
        };
        let channel_id: Uuid = row.get("id");
        let handle: String = row.get("handle");
        return Ok((channel_id, format!("dm:@{handle}")));
    }

    if let Ok(channel_id) = Uuid::parse_str(channel_ref.trim_start_matches("channel:")) {
        let row = sqlx::query("select name, kind from channels where id = $1")
            .bind(channel_id)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?;
        let Some(row) = row else {
            return Err(format!("unknown channel id: {channel_id}"));
        };
        let name: String = row.get("name");
        let kind: String = row.get("kind");
        return Ok((
            channel_id,
            if kind == "dm" {
                format!("dm:{name}")
            } else {
                format!("#{name}")
            },
        ));
    }

    let name = channel_ref.trim_start_matches('#');
    let row = sqlx::query("select id, name, kind from channels where lower(name) = lower($1)")
        .bind(name)
        .fetch_optional(pool)
        .await
        .map_err(to_string)?;
    let Some(row) = row else {
        return Err(format!("unknown channel: {channel_ref}"));
    };
    let channel_id: Uuid = row.get("id");
    let name: String = row.get("name");
    let kind: String = row.get("kind");
    Ok((
        channel_id,
        if kind == "dm" {
            format!("dm:{name}")
        } else {
            format!("#{name}")
        },
    ))
}

async fn resolve_agent_context_thread(
    pool: &PgPool,
    channel_id: Uuid,
    raw_thread: &str,
) -> CommandResult<Uuid> {
    let raw_thread = raw_thread.trim();
    if raw_thread.is_empty() {
        return Err("thread reference is empty".to_owned());
    }
    if let Ok(thread_id) = Uuid::parse_str(raw_thread) {
        return Ok(thread_id);
    }
    let pattern = format!("{raw_thread}%");
    let thread_id: Option<Uuid> = sqlx::query_scalar(
        r#"
        select id
        from messages
        where channel_id = $1 and id::text like $2
        order by created_at asc
        limit 1
        "#,
    )
    .bind(channel_id)
    .bind(pattern)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?;
    thread_id.ok_or_else(|| format!("unknown thread/message id in target: {raw_thread}"))
}

async fn resolve_agent_context_target(
    pool: &PgPool,
    raw_target: &str,
    thread_override: Option<&str>,
) -> CommandResult<AgentContextTarget> {
    let (channel_ref, thread_from_target) = split_context_target(raw_target);
    let (channel_id, channel_label) = resolve_agent_context_channel(pool, &channel_ref).await?;
    let thread_ref = thread_override
        .map(str::to_owned)
        .or(thread_from_target)
        .filter(|thread| !thread.trim().is_empty());
    let thread_root_id = match thread_ref {
        Some(thread_ref) => {
            Some(resolve_agent_context_thread(pool, channel_id, &thread_ref).await?)
        }
        None => None,
    };
    let label = match thread_root_id {
        Some(thread_root_id) => format!("{channel_label}:{}", short_id(thread_root_id)),
        None => channel_label,
    };
    Ok(AgentContextTarget {
        channel_id,
        thread_root_id,
        label,
    })
}

fn format_context_message_row(row: &sqlx::postgres::PgRow, include_channel: bool) -> String {
    let id: Uuid = row.get("id");
    let sender_name: String = row.get("sender_name");
    let sender_role: String = row.get("sender_role");
    let body: String = row.get("body");
    let created_at: DateTime<Utc> = row.get("created_at");
    let thread_root_id: Option<Uuid> = row.get("thread_root_id");
    let task_number: Option<i64> = row.get("task_number");
    let task_status: Option<String> = row.get("task_status");
    let body = compact_chars_middle(&body, AGENT_CONTEXT_TOOL_MESSAGE_LIMIT).replace('\n', "\n  ");
    let mut head = format!(
        "[{}] id={} sender={}({})",
        created_at.to_rfc3339(),
        short_id(id),
        sender_name,
        sender_role
    );
    if include_channel {
        let channel_name: String = row.get("channel_name");
        let channel_kind: String = row.get("channel_kind");
        if channel_kind == "dm" {
            head.push_str(&format!(" surface=dm:{channel_name}"));
        } else {
            head.push_str(&format!(" surface=#{channel_name}"));
        }
    }
    if let Some(thread_root_id) = thread_root_id {
        head.push_str(&format!(" thread={}", short_id(thread_root_id)));
    }
    if let Some(task_number) = task_number {
        head.push_str(&format!(
            " task=#{task_number}({})",
            task_status.unwrap_or_else(|| "unknown".to_owned())
        ));
    }
    let mut output = format!("{head}\n  {body}");
    if let Ok(attachment_summary) = row.try_get::<String, _>("attachment_summary") {
        if !attachment_summary.trim().is_empty() {
            output.push_str("\n  attachments:");
            for line in attachment_summary.lines() {
                output.push_str("\n  - ");
                output.push_str(line);
            }
            output.push_str(
                "\n  To inspect an attachment, run attachment-info with its attachment_id.",
            );
        }
    }
    output
}

async fn agent_context_history_read(pool: &PgPool, args: &[String]) -> CommandResult<String> {
    let target = arg_value(args, "--target")
        .or_else(|| arg_value(args, "--channel"))
        .ok_or_else(|| "history-read requires --target \"#channel[:thread]\"".to_owned())?;
    let limit = parse_context_tool_limit(args, 30, 100)?;
    let thread_override = arg_value(args, "--thread");
    let target = resolve_agent_context_target(pool, &target, thread_override.as_deref()).await?;

    let rows = if let Some(thread_root_id) = target.thread_root_id {
        sqlx::query(&format!(
            r#"
            select
                m.id, m.sender_name, m.sender_role, m.body, m.thread_root_id, m.created_at,
                t.number as task_number, t.status as task_status,
                {}
            from messages m
            left join tasks t on t.message_id = m.id
            where m.channel_id = $1
              and (m.id = $2 or m.thread_root_id = $2)
            order by m.created_at desc
            limit $3
            "#,
            attachment_summary_sql()
        ))
        .bind(target.channel_id)
        .bind(thread_root_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(to_string)?
    } else {
        sqlx::query(&format!(
            r#"
            select
                m.id, m.sender_name, m.sender_role, m.body, m.thread_root_id, m.created_at,
                t.number as task_number, t.status as task_status,
                {}
            from messages m
            left join tasks t on t.message_id = m.id
            where m.channel_id = $1
              and m.thread_root_id is null
            order by m.created_at desc
            limit $2
            "#,
            attachment_summary_sql()
        ))
        .bind(target.channel_id)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(to_string)?
    };

    let mut output = vec![format!(
        "LocalSlock history for {} ({} message{})",
        target.label,
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    )];
    for row in rows.into_iter().rev() {
        output.push(format_context_message_row(&row, false));
    }
    Ok(output.join("\n\n"))
}

async fn agent_context_message_search(pool: &PgPool, args: &[String]) -> CommandResult<String> {
    let query = arg_value(args, "--query")
        .or_else(|| arg_value(args, "-q"))
        .ok_or_else(|| "message-search requires --query <text>".to_owned())?;
    let query = query.trim();
    if query.is_empty() {
        return Err("message-search query is empty".to_owned());
    }
    let limit = parse_context_tool_limit(args, 30, 100)?;
    let target = match arg_value(args, "--target").or_else(|| arg_value(args, "--channel")) {
        Some(target) => Some(resolve_agent_context_target(pool, &target, None).await?),
        None => None,
    };
    let pattern = format!("%{query}%");

    let rows = if let Some(target) = target {
        sqlx::query(&format!(
            r#"
            select
                m.id, m.sender_name, m.sender_role, m.body, m.thread_root_id, m.created_at,
                c.name as channel_name, c.kind as channel_kind,
                t.number as task_number, t.status as task_status,
                {}
            from messages m
            join channels c on c.id = m.channel_id
            left join tasks t on t.message_id = m.id
            where m.channel_id = $1
              and m.body ilike $2
            order by m.created_at desc
            limit $3
            "#,
            attachment_summary_sql()
        ))
        .bind(target.channel_id)
        .bind(pattern)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(to_string)?
    } else {
        sqlx::query(&format!(
            r#"
            select
                m.id, m.sender_name, m.sender_role, m.body, m.thread_root_id, m.created_at,
                c.name as channel_name, c.kind as channel_kind,
                t.number as task_number, t.status as task_status,
                {}
            from messages m
            join channels c on c.id = m.channel_id
            left join tasks t on t.message_id = m.id
            where m.body ilike $1
            order by m.created_at desc
            limit $2
            "#,
            attachment_summary_sql()
        ))
        .bind(pattern)
        .bind(limit)
        .fetch_all(pool)
        .await
        .map_err(to_string)?
    };

    let mut output = vec![format!(
        "LocalSlock message search for {:?} ({} result{})",
        query,
        rows.len(),
        if rows.len() == 1 { "" } else { "s" }
    )];
    for row in rows {
        output.push(format_context_message_row(&row, true));
    }
    Ok(output.join("\n\n"))
}

async fn agent_context_attachment_info(pool: &PgPool, args: &[String]) -> CommandResult<String> {
    let raw_id = arg_value(args, "--attachment-id")
        .or_else(|| arg_value(args, "--id"))
        .ok_or_else(|| "attachment-info requires --attachment-id <uuid>".to_owned())?;
    let attachment_id =
        Uuid::parse_str(raw_id.trim()).map_err(|err| format!("invalid attachment id: {err}"))?;
    let row = sqlx::query(
        r#"
        select
            ma.id,
            ma.message_id,
            ma.original_name,
            ma.mime_type,
            ma.size_bytes,
            ma.storage_path,
            ma.created_at,
            m.channel_id,
            m.thread_root_id,
            c.name as channel_name,
            c.kind as channel_kind
        from message_attachments ma
        join messages m on m.id = ma.message_id
        join channels c on c.id = m.channel_id
        where ma.id = $1
        "#,
    )
    .bind(attachment_id)
    .fetch_optional(pool)
    .await
    .map_err(to_string)?
    .ok_or_else(|| format!("attachment {attachment_id} does not exist"))?;

    let mime_type: String = row.get("mime_type");
    let storage_path: String = row.get("storage_path");
    let exists = PathBuf::from(&storage_path).exists();
    let channel_name: String = row.get("channel_name");
    let channel_kind: String = row.get("channel_kind");
    let surface = if channel_kind == "dm" {
        format!("dm:{channel_name}")
    } else {
        format!("#{channel_name}")
    };
    let mut output = vec![
        format!("LocalSlock attachment {}", row.get::<Uuid, _>("id")),
        format!("message_id={}", row.get::<Uuid, _>("message_id")),
        format!("surface={surface}"),
        format!("name=\"{}\"", row.get::<String, _>("original_name")),
        format!("mime={mime_type}"),
        format!("size={}", format_attachment_size(row.get("size_bytes"))),
        format!("local_path=\"{storage_path}\""),
        format!("file_exists={exists}"),
    ];
    if mime_type.starts_with("image/") {
        output.push(
            "vision_hint=This is an image attachment. Inspect local_path directly with your runtime's file/vision support before answering visual UI questions."
                .to_owned(),
        );
    }
    Ok(output.join("\n"))
}

async fn agent_context_agent_inspect(pool: &PgPool, args: &[String]) -> CommandResult<String> {
    let target = arg_value(args, "--target")
        .or_else(|| arg_value(args, "--agent"))
        .ok_or_else(|| "agent-inspect requires --target @handle".to_owned())?;
    let agent_id = resolve_agent_by_handle(pool, &target).await?;
    let agent = sqlx::query(
        r#"
        select handle, display_name, role, status, runtime, model, avatar, description,
               working_directory, daily_budget_micros
        from agents
        where id = $1
        "#,
    )
    .bind(agent_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;
    let handle: String = agent.get("handle");
    let mut output = vec![
        format!("Agent @{handle}"),
        format!("display_name={}", agent.get::<String, _>("display_name")),
        format!("role={}", agent.get::<String, _>("role")),
        format!("status={}", agent.get::<String, _>("status")),
        format!(
            "runtime={}/{}",
            agent.get::<String, _>("runtime"),
            agent.get::<String, _>("model")
        ),
        format!("description={}", agent.get::<String, _>("description")),
        format!(
            "working_directory={}",
            agent.get::<String, _>("working_directory")
        ),
        format!(
            "daily_budget=${:.4}",
            agent.get::<i64, _>("daily_budget_micros") as f64 / 1_000_000.0
        ),
    ];

    let runs = sqlx::query(
        r#"
        select status, command, input_tokens, output_tokens, cost_micros, started_at, stopped_at
        from agent_runs
        where agent_id = $1
        order by started_at desc
        limit 5
        "#,
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    if !runs.is_empty() {
        output.push("recent_runs:".to_owned());
        for row in runs {
            let started_at: DateTime<Utc> = row.get("started_at");
            let stopped_at: Option<DateTime<Utc>> = row.get("stopped_at");
            output.push(format!(
                "- {} status={} tokens={}/{} cost=${:.4} command=\"{}\" stopped={}",
                started_at.to_rfc3339(),
                row.get::<String, _>("status"),
                row.get::<i64, _>("input_tokens"),
                row.get::<i64, _>("output_tokens"),
                row.get::<i64, _>("cost_micros") as f64 / 1_000_000.0,
                compact_chars_middle(&row.get::<String, _>("command"), 120).replace('"', "\\\""),
                stopped_at
                    .map(|value| value.to_rfc3339())
                    .unwrap_or_else(|| "active".to_owned())
            ));
        }
    }

    let work_items = sqlx::query(
        r#"
        select source_kind, title, status, created_at, updated_at
        from agent_work_items
        where agent_id = $1
        order by created_at desc
        limit 5
        "#,
    )
    .bind(agent_id)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    if !work_items.is_empty() {
        output.push("recent_requests:".to_owned());
        for row in work_items {
            let created_at: DateTime<Utc> = row.get("created_at");
            output.push(format!(
                "- {} [{}] {} status={}",
                created_at.to_rfc3339(),
                row.get::<String, _>("source_kind"),
                compact_chars_middle(&row.get::<String, _>("title"), 120).replace('\n', " "),
                row.get::<String, _>("status")
            ));
        }
    }

    let activities = sqlx::query(
        r#"
        select phase, status, summary, created_at
        from agent_activities
        where agent_id = $1 or agent_handle = $2
        order by created_at desc
        limit 5
        "#,
    )
    .bind(agent_id)
    .bind(&handle)
    .fetch_all(pool)
    .await
    .map_err(to_string)?;
    if !activities.is_empty() {
        output.push("recent_activity:".to_owned());
        for row in activities {
            let created_at: DateTime<Utc> = row.get("created_at");
            output.push(format!(
                "- {} {}:{} {}",
                created_at.to_rfc3339(),
                row.get::<String, _>("phase"),
                row.get::<String, _>("status"),
                compact_chars_middle(&row.get::<String, _>("summary"), 120).replace('\n', " ")
            ));
        }
    }

    Ok(output.join("\n"))
}

async fn agent_context_artifact_read_in_pool(
    pool: &PgPool,
    args: &[String],
) -> CommandResult<String> {
    let raw_id = arg_value(args, "--artifact-id")
        .or_else(|| arg_value(args, "--id"))
        .ok_or_else(|| "artifact-read requires --artifact-id <uuid>".to_owned())?;
    let artifact_id =
        Uuid::parse_str(raw_id.trim()).map_err(|err| format!("invalid artifact id: {err}"))?;
    let artifact = load_artifact(pool, artifact_id).await?;
    Ok(format!(
        "LocalSlock artifact {}\nkind={}\ntitle={}\nsummary={}\nmessage_id={}\nchannel_id={}\nthread_root_id={}\ncreator=@{}\nmetadata={}\n\n{}",
        artifact.id,
        artifact.kind,
        artifact.title,
        artifact.summary,
        artifact.message_id,
        artifact.channel_id,
        artifact
            .thread_root_id
            .map(|value| value.to_string())
            .unwrap_or_else(|| "none".to_owned()),
        artifact.creator_agent_handle.unwrap_or_else(|| "unknown".to_owned()),
        artifact.metadata,
        artifact.content
    ))
}

async fn run_agent_context_tool(args: &[String]) -> CommandResult<String> {
    if args.is_empty() || has_arg(args, "--help") || has_arg(args, "-h") {
        return Ok(
            "LocalSlock agent context tool\n\nCommands:\n  history-read --target \"#channel[:thread]\" [--limit 30]\n  message-search --query <text> [--target \"#channel\"] [--limit 30]\n  attachment-info --attachment-id <uuid>\n  artifact-read --artifact-id <uuid>\n  agent-inspect --target @handle\n\nTargets may be #channel, #channel:<message-id-prefix>, dm:@agent, or a channel UUID."
                .to_owned(),
        );
    }

    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&db_url())
        .await
        .map_err(to_string)?;
    match args[0].as_str() {
        "history-read" | "read-history" | "read" => agent_context_history_read(&pool, args).await,
        "message-search" | "search-messages" | "search" => {
            agent_context_message_search(&pool, args).await
        }
        "attachment-info" | "attachment" | "attachment-view" => {
            agent_context_attachment_info(&pool, args).await
        }
        "agent-inspect" | "inspect-agent" | "agent-query" => {
            agent_context_agent_inspect(&pool, args).await
        }
        "artifact-read" | "artifact" | "artifact-view" => {
            agent_context_artifact_read_in_pool(&pool, args).await
        }
        other => Err(format!("unknown agent context tool command: {other}")),
    }
}

pub fn run() {
    let database_url = db_url();
    let pool = tauri::async_runtime::block_on(async {
        PgPoolOptions::new()
            .max_connections(5)
            .connect(&database_url)
            .await
    })
    .expect("failed to connect LocalSlock Postgres database");

    tauri::async_runtime::block_on(migrate(&pool)).expect("failed to initialize LocalSlock schema");
    spawn_supervisor_process(&database_url);
    let state_db_url = database_url.clone();
    let reminder_pool = pool.clone();

    tauri::Builder::default()
        .manage(AppState {
            pool,
            db_url: state_db_url,
        })
        .setup(move |app| {
            spawn_ui_refresh_listener(app.handle().clone(), database_url.clone());
            spawn_reminder_worker(reminder_pool.clone());
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("LocalSlock");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            artifact_read,
            cancel_agent_work,
            cancel_reminder,
            check_runtime,
            complete_reminder,
            create_agent,
            create_agent_schedule,
            create_channel,
            create_reminder,
            claim_task,
            delete_agent,
            delete_channel,
            delete_message,
            dispatch_agent_work,
            install_supervisor_service,
            mark_channel_read,
            open_dm_with_agent,
            retry_agent_work,
            send_message,
            set_channel_agent_membership,
            snooze_reminder,
            start_agent,
            stop_agent,
            uninstall_supervisor_service,
            update_agent,
            update_agent_schedule_status,
            update_channel,
            update_message,
            update_thread_followed,
            update_task_title,
            update_task_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running LocalSlock");
}

fn main() {
    let args = env::args().collect::<Vec<_>>();
    if let Some(tool_arg_index) = args.iter().position(|arg| arg == "--agent-context-tool") {
        let tool_args = args
            .get(tool_arg_index + 1..)
            .map(|args| args.to_vec())
            .unwrap_or_default();
        match tauri::async_runtime::block_on(run_agent_context_tool(&tool_args)) {
            Ok(output) => {
                println!("{output}");
                return;
            }
            Err(err) => {
                eprintln!("{err}");
                std::process::exit(1);
            }
        }
    }

    if args.iter().any(|arg| arg == "--supervisor") {
        if let Err(err) = tauri::async_runtime::block_on(run_supervisor()) {
            eprintln!("LocalSlock supervisor stopped: {err}");
        }
    } else {
        run();
    }
}

#[cfg(test)]
mod tests {
    use super::{
        agent_context_agent_inspect, agent_context_artifact_read_in_pool,
        agent_context_attachment_info, agent_context_history_read, agent_context_message_search,
        append_streaming_agent_message, capped_stream_delta, claim_next_supervisor_command,
        claude_message_text, claude_result_error, claude_stream_event_activity,
        claude_system_prompt, claude_text_delta, codex_item_started_activity,
        codex_turn_id_from_value, consume_streaming_agent_control_lines, delete_agent_in_pool,
        delete_channel_in_pool, extract_agent_event_json, extract_agent_mentions,
        finish_streaming_agent_message, handle_agent_event, insert_agent_message,
        load_agent_memory_context, load_channel_agent_roster, load_messages, load_reminders,
        load_runtime_thread_id, maybe_hide_silent_streaming_reply, migrate,
        open_dm_with_agent_in_pool, parse_activity_metadata, process_due_agent_schedules,
        process_due_reminders, queue_mentions_as_work_items, record_agent_activity,
        send_owner_message_in_pool, short_id, silent_reply_reason,
        upsert_agent_thread_subscription, upsert_runtime_thread_id, usage_from_run_log,
        usage_from_runtime_event, AgentEvent, MentionDispatchOrigin, AGENT_MEMORY_CONTEXT_LIMIT,
        DEFAULT_DATABASE_URL, STREAMING_MESSAGE_BODY_LIMIT, STREAMING_TRUNCATION_MARKER,
    };
    use chrono::{DateTime, Duration as ChronoDuration, Utc};
    use serde_json::json;
    use sqlx::{postgres::PgPoolOptions, PgPool, Row};
    use uuid::Uuid;

    #[test]
    fn extracts_unique_agent_mentions() {
        let mentions = extract_agent_mentions("ping @Hancock and @agent-2, then @Hancock again");
        assert_eq!(mentions, vec!["Hancock", "agent-2"]);
    }

    #[test]
    fn ignores_empty_or_email_like_at_signs() {
        let mentions = extract_agent_mentions("email a@b.com and a lone @ sign");
        assert!(mentions.is_empty());
    }

    #[test]
    fn memory_context_is_bounded_and_preserves_tail() {
        let dir = std::env::temp_dir().join(format!("localslock-memory-test-{}", Uuid::new_v4()));
        std::fs::create_dir_all(&dir).expect("create temp memory dir");
        let memory_path = dir.join("MEMORY.md");
        let memory = format!(
            "# Agent\n\n{}\n\n## Active Context\nimportant tail survives",
            "context ".repeat(AGENT_MEMORY_CONTEXT_LIMIT)
        );
        std::fs::write(&memory_path, memory).expect("write memory");

        let context =
            load_agent_memory_context(dir.to_str().expect("utf8 temp dir")).expect("load memory");
        std::fs::remove_dir_all(&dir).ok();

        let context = context.expect("memory should load");
        assert!(context.contains("LocalSlock omitted"));
        assert!(context.contains("important tail survives"));
        assert!(context.chars().count() < AGENT_MEMORY_CONTEXT_LIMIT + 1_000);
    }

    #[test]
    fn runtime_standing_prompt_carries_memory_once() {
        let prompt =
            claude_system_prompt("tester", Some("Persistent memory: prefer concise replies"));
        assert!(prompt.contains("one warm runtime session per agent"));
        assert!(prompt.contains("channel and thread are delivered as message envelope fields"));
        assert!(prompt.contains("Persistent memory: prefer concise replies"));
    }

    #[tokio::test]
    async fn agent_context_tool_reads_thread_history_and_searches_messages() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let channel_id = insert_test_channel(&pool, "context-tools").await?;
            let root_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', 'root message with needle', false)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            sqlx::query(
                r#"
                insert into messages (channel_id, thread_root_id, sender_name, sender_role, body, is_task)
                values ($1, $2, 'agent', 'agent', 'reply inside thread', false),
                       ($1, null, 'Dylan', 'owner', 'separate root message', false)
                "#,
            )
            .bind(channel_id)
            .bind(root_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let history_args = vec![
                "history-read".to_owned(),
                "--target".to_owned(),
                format!("#context-tools:{}", short_id(root_id)),
                "--limit".to_owned(),
                "10".to_owned(),
            ];
            let history = agent_context_history_read(&pool, &history_args).await?;
            assert!(history.contains("root message with needle"));
            assert!(history.contains("reply inside thread"));
            assert!(!history.contains("separate root message"));

            let search_args = vec![
                "message-search".to_owned(),
                "--query".to_owned(),
                "needle".to_owned(),
                "--target".to_owned(),
                "#context-tools".to_owned(),
            ];
            let search = agent_context_message_search(&pool, &search_args).await?;
            assert!(search.contains("root message with needle"));
            assert!(search.contains("surface=#context-tools"));
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn artifact_create_event_persists_artifact_and_message_card() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "artifact-agent").await?;
            let channel_id = insert_test_channel(&pool, "artifacts").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, status)
                values ($1, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::ArtifactCreate {
                    channel: None,
                    channel_id: Some(channel_id),
                    thread_root_id: None,
                    kind: "markdown".to_owned(),
                    title: "Review report".to_owned(),
                    summary: Some("Two findings and one follow-up.".to_owned()),
                    content: "# Review report\n\n- finding".to_owned(),
                    metadata: Some(json!({"source": "test"})),
                },
            )
            .await?;

            let artifact = sqlx::query(
                r#"
                select id, message_id, kind, title, summary, content, metadata
                from artifacts
                where channel_id = $1
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let artifact_id: Uuid = artifact.get("id");
            let message_id: Uuid = artifact.get("message_id");
            assert_eq!(artifact.get::<String, _>("kind"), "markdown");
            assert_eq!(artifact.get::<String, _>("title"), "Review report");
            assert_eq!(
                artifact.get::<String, _>("summary"),
                "Two findings and one follow-up."
            );

            let messages = load_messages(&pool).await?;
            let message = messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("artifact message should be loaded");
            assert!(message.body.contains("Created artifact: Review report"));
            assert_eq!(message.artifacts.len(), 1);
            assert_eq!(message.artifacts[0].id, artifact_id);

            let tool_output = agent_context_artifact_read_in_pool(
                &pool,
                &[
                    "artifact-read".to_owned(),
                    "--artifact-id".to_owned(),
                    artifact_id.to_string(),
                ],
            )
            .await?;
            assert!(tool_output.contains("kind=markdown"));
            assert!(tool_output.contains("# Review report"));
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[test]
    fn extracts_plain_agent_event_lines() {
        assert_eq!(
            extract_agent_event_json(r#"LOCAL_SLOCK_EVENT {"type":"message","body":"ok"}"#),
            Some(r#"{"type":"message","body":"ok"}"#)
        );
    }

    #[test]
    fn extracts_codex_wrapped_agent_event_lines() {
        assert_eq!(
            extract_agent_event_json(
                r#"[stderr] LOCAL_SLOCK_EVENT {"type":"message","body":"from tool output"}"#
            ),
            Some(r#"{"type":"message","body":"from tool output"}"#)
        );
    }

    #[test]
    fn extracts_stdout_wrapped_agent_event_lines() {
        assert_eq!(
            extract_agent_event_json(
                r#"[stdout] LOCAL_SLOCK_EVENT {"type":"message","body":"from final output"}"#
            ),
            Some(r#"{"type":"message","body":"from final output"}"#)
        );
    }

    #[test]
    fn ignores_event_examples_embedded_in_instructions() {
        assert!(extract_agent_event_json(
            r#"[stderr] Reply with: LOCAL_SLOCK_EVENT {"type":"message","body":"..."}"#
        )
        .is_none());
    }

    #[test]
    fn caps_streaming_deltas_with_marker() {
        let remaining = STREAMING_MESSAGE_BODY_LIMIT - 4;
        let delta = "x".repeat(remaining + 16);
        let (capped, truncated) = capped_stream_delta(&delta, 4);
        assert!(truncated);
        assert!(capped.ends_with(STREAMING_TRUNCATION_MARKER));
        assert_eq!(capped.chars().count() + 4, STREAMING_MESSAGE_BODY_LIMIT);
    }

    #[test]
    fn detects_silent_reply_marker() {
        assert_eq!(
            silent_reply_reason("LOCAL_SLOCK_SILENT_REPLY: greeting only"),
            Some("greeting only".to_owned())
        );
        assert_eq!(
            silent_reply_reason("LOCAL_SLOCK_SILENT_REPLY"),
            Some(String::new())
        );
        assert_eq!(silent_reply_reason("LOCAL_SLOCK_SILENT_REPLYING"), None);
    }

    #[test]
    fn structures_activity_metadata_from_detail() {
        let metadata = parse_activity_metadata("pid=123, thread_id=abc, duration=42 ms");
        assert_eq!(metadata["pid"], "123");
        assert_eq!(metadata["thread_id"], "abc");
        assert_eq!(metadata["duration_ms"], 42);
    }

    #[test]
    fn extracts_codex_turn_ids_from_response_and_notification() {
        assert_eq!(
            codex_turn_id_from_value(&json!({"result": {"turn": {"id": "turn-1"}}})),
            Some("turn-1".to_owned())
        );
        assert_eq!(
            codex_turn_id_from_value(&json!({"params": {"turn": {"id": "turn-2"}}})),
            Some("turn-2".to_owned())
        );
        assert_eq!(
            codex_turn_id_from_value(&json!({"params": {"turnId": "turn-3"}})),
            Some("turn-3".to_owned())
        );
    }

    #[test]
    fn maps_codex_command_and_file_activity() {
        assert_eq!(
            codex_item_started_activity(&json!({
                "params": {
                    "item": {
                        "type": "commandExecution",
                        "command": "cargo test"
                    }
                }
            })),
            (
                "command",
                "Running command",
                json!({"command": "cargo test"}).to_string()
            )
        );
        assert_eq!(
            codex_item_started_activity(&json!({
                "params": {
                    "item": {
                        "type": "fileChange",
                        "path": "src/main.rs",
                        "operation": "update"
                    }
                }
            })),
            (
                "file_edit",
                "Editing file",
                json!({"file": "src/main.rs", "operation": "update"}).to_string()
            )
        );
    }

    #[test]
    fn parses_codex_thread_token_usage_events() {
        let value = json!({
            "method": "thread/tokenUsage/updated",
            "params": {
                "tokenUsage": {
                    "total": {
                        "inputTokens": 11488567,
                        "outputTokens": 36332
                    },
                    "last": {
                        "inputTokens": 33569,
                        "cachedInputTokens": 31616,
                        "outputTokens": 1278
                    }
                }
            }
        });
        assert_eq!(usage_from_runtime_event(&value), Some((33569, 1278)));

        let log = format!(
            "[codex] {{\"method\":\"item/agentMessage/delta\",\"params\":{{\"delta\":\"hi\"}}}}\n[codex] {value}"
        );
        assert_eq!(usage_from_run_log(&log), Some((33569, 1278)));
    }

    #[test]
    fn parses_claude_stream_json_events() {
        assert_eq!(
            claude_text_delta(
                &json!({"type": "stream_event", "event": {"delta": {"type": "text_delta", "text": "hi"}}})
            ),
            Some("hi")
        );
        assert_eq!(
            claude_message_text(&json!({
                "type": "assistant",
                "message": {
                    "content": [
                        {"type": "text", "text": "hello"},
                        {"type": "tool_use", "name": "Read"}
                    ]
                }
            })),
            Some("hello".to_owned())
        );
        assert_eq!(
            claude_result_error(
                &json!({"type": "result", "is_error": true, "result": "rate limited"})
            ),
            Some("rate limited".to_owned())
        );
        assert_eq!(
            claude_result_error(
                &json!({"type": "result", "is_error": false, "api_error_status": null, "result": "ok"})
            ),
            None
        );
        assert_eq!(
            claude_result_error(
                &json!({"type": "result", "is_error": false, "api_error_status": "rate_limited", "result": "busy"})
            ),
            Some("rate_limited".to_owned())
        );
        assert_eq!(
            claude_stream_event_activity(&json!({"type": "system", "subtype": "init"})),
            Some(("run", "Runtime ready", "Claude stream connected".to_owned()))
        );
        assert_eq!(
            claude_stream_event_activity(
                &json!({"type": "rate_limit_event", "rate_limit_info": {"status": "allowed"}})
            ),
            None
        );
        assert_eq!(
            claude_stream_event_activity(
                &json!({"type": "stream_event", "event": {"type": "message_stop"}})
            ),
            None
        );
        assert_eq!(
            claude_stream_event_activity(
                &json!({"type": "stream_event", "event": {"type": "content_block_start", "content_block": {"type": "tool_use", "name": "Bash"}}})
            ),
            Some((
                "command",
                "Running command",
                json!({"tool": "Bash"}).to_string()
            ))
        );
        assert_eq!(
            claude_stream_event_activity(
                &json!({"type": "stream_event", "event": {"type": "content_block_start", "content_block": {"type": "tool_use", "name": "Edit"}}})
            ),
            Some((
                "file_edit",
                "Editing file",
                json!({"tool": "Edit"}).to_string()
            ))
        );
    }

    async fn test_pool() -> Option<(PgPool, String)> {
        test_pool_with_connections(1).await
    }

    async fn test_pool_with_connections(max_connections: u32) -> Option<(PgPool, String)> {
        let database_url = std::env::var("LOCAL_SLOCK_TEST_DATABASE_URL")
            .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        let bootstrap_pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect(&database_url)
            .await
        {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!("skipping postgres-backed LocalSlock DM test: {err}");
                return None;
            }
        };
        let schema = format!("localslock_test_{}", Uuid::new_v4().simple());
        if let Err(err) = sqlx::query(&format!(r#"create schema "{schema}""#))
            .execute(&bootstrap_pool)
            .await
        {
            eprintln!("skipping postgres-backed LocalSlock DM test: {err}");
            bootstrap_pool.close().await;
            return None;
        }
        bootstrap_pool.close().await;

        let schema_for_hook = schema.clone();
        let pool = match PgPoolOptions::new()
            .max_connections(max_connections)
            .after_connect(move |conn, _meta| {
                let schema = schema_for_hook.clone();
                Box::pin(async move {
                    sqlx::query(&format!(r#"set search_path to "{schema}", public"#))
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&database_url)
            .await
        {
            Ok(pool) => pool,
            Err(err) => {
                eprintln!("skipping postgres-backed LocalSlock DM test: {err}");
                return None;
            }
        };
        if let Err(err) = migrate(&pool).await {
            eprintln!("skipping postgres-backed LocalSlock DM test: {err}");
            let _ = sqlx::query(&format!(r#"drop schema if exists "{schema}" cascade"#))
                .execute(&pool)
                .await;
            pool.close().await;
            return None;
        }
        Some((pool, schema))
    }

    async fn drop_test_schema(pool: PgPool, schema: String) {
        let _ = sqlx::query(&format!(r#"drop schema if exists "{schema}" cascade"#))
            .execute(&pool)
            .await;
        pool.close().await;
    }

    async fn insert_test_agent(pool: &PgPool, handle: &str) -> Result<Uuid, String> {
        sqlx::query_scalar(
            r#"
            insert into agents (handle, display_name, role, status, runtime, model, avatar, description)
            values ($1, $2, 'agent', 'idle', 'codex', 'gpt-5.5', 'D', 'test agent')
            returning id
            "#,
        )
        .bind(handle)
        .bind(handle)
        .fetch_one(pool)
        .await
        .map_err(|err| err.to_string())
    }

    async fn insert_test_channel(pool: &PgPool, name: &str) -> Result<Uuid, String> {
        sqlx::query_scalar(
            r#"
            insert into channels (name, description, kind)
            values ($1, 'test channel', 'channel')
            returning id
            "#,
        )
        .bind(name)
        .fetch_one(pool)
        .await
        .map_err(|err| err.to_string())
    }

    #[tokio::test]
    async fn usage_event_updates_run_tokens_and_cost() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "usage-agent").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, status)
                values ($1, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::Usage {
                    input_tokens: Some(1000),
                    output_tokens: Some(200),
                    total_tokens: None,
                    cost_micros: Some(1234),
                    cost_usd: None,
                },
            )
            .await?;
            let row = sqlx::query(
                "select input_tokens, output_tokens, cost_micros from agent_runs where id = $1",
            )
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(row.get::<i64, _>("input_tokens"), 1000);
            assert_eq!(row.get::<i64, _>("output_tokens"), 200);
            assert_eq!(row.get::<i64, _>("cost_micros"), 1234);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn memory_events_append_and_compact_memory_file() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "memory-agent").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, status)
                values ($1, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let dir =
                std::env::temp_dir().join(format!("localslock-memory-write-{}", Uuid::new_v4()));
            sqlx::query("update agents set working_directory = $2 where id = $1")
                .bind(agent_id)
                .bind(dir.to_string_lossy().to_string())
                .execute(&pool)
                .await
                .map_err(|err| err.to_string())?;
            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::MemoryAppend {
                    body: "Remember: concise replies.".to_owned(),
                },
            )
            .await?;
            let memory_path = dir.join("MEMORY.md");
            let memory = std::fs::read_to_string(&memory_path).map_err(|err| err.to_string())?;
            assert!(memory.contains("Remember: concise replies."));
            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::MemoryCompact {
                    body: "# @memory-agent\n\n## Role\nCompact memory.\n".to_owned(),
                },
            )
            .await?;
            let memory = std::fs::read_to_string(&memory_path).map_err(|err| err.to_string())?;
            assert_eq!(memory, "# @memory-agent\n\n## Role\nCompact memory.\n");
            let _ = std::fs::remove_dir_all(dir);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn channel_events_create_channel_and_invite_agents() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "creator-agent").await?;
            let reviewer_id = insert_test_agent(&pool, "reviewer-agent").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, status)
                values ($1, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::ChannelCreate {
                    name: "Feature Room".to_owned(),
                    description: Some("long-lived feature coordination".to_owned()),
                    agent_handles: Some(vec!["@reviewer-agent".to_owned()]),
                },
            )
            .await?;
            let channel_id: Uuid =
                sqlx::query_scalar("select id from channels where name = 'feature-room'")
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            let members: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from channel_members
                where channel_id = $1 and agent_id = any($2)
                "#,
            )
            .bind(channel_id)
            .bind(&[agent_id, reviewer_id])
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(members, 2);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn profile_update_event_updates_agent_profile() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "profile-agent").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, status)
                values ($1, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::ProfileUpdate {
                    display_name: Some("Profile Agent".to_owned()),
                    role: Some("vision reviewer".to_owned()),
                    avatar: Some("P".to_owned()),
                    description: Some("Reviews screenshots and agent handoffs.".to_owned()),
                },
            )
            .await?;

            let row = sqlx::query(
                "select display_name, role, avatar, description from agents where id = $1",
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(row.get::<String, _>("display_name"), "Profile Agent");
            assert_eq!(row.get::<String, _>("role"), "vision reviewer");
            assert_eq!(row.get::<String, _>("avatar"), "P");
            assert_eq!(
                row.get::<String, _>("description"),
                "Reviews screenshots and agent handoffs."
            );
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn history_context_and_attachment_tool_expose_attachment_paths() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let channel_id = insert_test_channel(&pool, "vision-channel").await?;
            let message_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', 'Please inspect this screenshot', false)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let image_path =
                std::env::temp_dir().join(format!("localslock-vision-{}.png", Uuid::new_v4()));
            std::fs::write(&image_path, b"fake image bytes").map_err(|err| err.to_string())?;
            let attachment_id: Uuid = sqlx::query_scalar(
                r#"
                insert into message_attachments (
                    message_id, original_name, mime_type, size_bytes, storage_path
                )
                values ($1, 'screen.png', 'image/png', 16, $2)
                returning id
                "#,
            )
            .bind(message_id)
            .bind(image_path.to_string_lossy().to_string())
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let history = agent_context_history_read(
                &pool,
                &[
                    "history-read".to_owned(),
                    "--target".to_owned(),
                    "#vision-channel".to_owned(),
                ],
            )
            .await?;
            assert!(history.contains(&attachment_id.to_string()));
            assert!(history.contains("local_path="));
            assert!(history.contains("attachment-info"));

            let info = agent_context_attachment_info(
                &pool,
                &[
                    "attachment-info".to_owned(),
                    "--attachment-id".to_owned(),
                    attachment_id.to_string(),
                ],
            )
            .await?;
            assert!(info.contains("mime=image/png"));
            assert!(info.contains("file_exists=true"));
            assert!(info.contains("vision_hint="));
            let _ = std::fs::remove_file(image_path);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn agent_inspect_tool_summarizes_profile_runs_requests_and_activity() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "inspectable").await?;
            let channel_id = insert_test_channel(&pool, "inspect-channel").await?;
            sqlx::query(
                r#"
                update agents
                set display_name = 'Inspectable Agent',
                    role = 'review specialist',
                    description = 'Reviews work from other agents.',
                    daily_budget_micros = 2500000
                where id = $1
                "#,
            )
            .bind(agent_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (
                    agent_id, command, status, input_tokens, output_tokens, cost_micros
                )
                values ($1, 'codex app-server', 'complete', 100, 20, 500)
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            sqlx::query(
                r#"
                insert into agent_work_items (agent_id, channel_id, title, context, status, source_kind)
                values ($1, $2, 'Review implementation', 'context', 'done', 'collaboration')
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;
            record_agent_activity(
                &pool,
                Some(agent_id),
                Some(run_id),
                "thinking",
                "Reading recent context",
                "{}".to_owned(),
            )
            .await?;

            let output = agent_context_agent_inspect(
                &pool,
                &[
                    "agent-inspect".to_owned(),
                    "--target".to_owned(),
                    "@inspectable".to_owned(),
                ],
            )
            .await?;
            assert!(output.contains("Agent @inspectable"));
            assert!(output.contains("display_name=Inspectable Agent"));
            assert!(output.contains("role=review specialist"));
            assert!(output.contains("recent_runs:"));
            assert!(output.contains("recent_requests:"));
            assert!(output.contains("recent_activity:"));
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn streaming_agent_messages_append_and_finish() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "streamer").await?;
            let channel_id = insert_test_channel(&pool, "streaming").await?;
            let stream_key = "run-1:item-1";

            let message_id = append_streaming_agent_message(
                &pool, agent_id, channel_id, None, stream_key, "Hel",
            )
            .await?;
            let same_message_id =
                append_streaming_agent_message(&pool, agent_id, channel_id, None, stream_key, "lo")
                    .await?;
            assert_eq!(message_id, same_message_id);
            finish_streaming_agent_message(&pool, stream_key, "complete").await?;

            let messages = load_messages(&pool).await?;
            let message = messages
                .iter()
                .find(|message| message.id == message_id)
                .expect("streaming message should be visible in bootstrap payload");
            assert_eq!(message.body, "Hello");
            assert_eq!(message.delivery_state, "complete");
            assert_eq!(message.stream_key, stream_key);

            upsert_runtime_thread_id(&pool, agent_id, "codex", "thread-1", "idle").await?;
            assert_eq!(
                load_runtime_thread_id(&pool, agent_id, "codex").await?,
                Some("thread-1".to_owned())
            );
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        result.unwrap();
    }

    #[tokio::test]
    async fn silent_streaming_reply_hides_message_and_marks_work_item() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "silent-agent").await?;
            let channel_id = insert_test_channel(&pool, "silent-channel").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, status)
                values ($1, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let work_item_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_work_items (agent_id, channel_id, title, context, status, run_id)
                values ($1, $2, 'hello', 'hi', 'running', $3)
                returning id
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let stream_key = "silent-run:item-1";
            let message_id = append_streaming_agent_message(
                &pool,
                agent_id,
                channel_id,
                None,
                stream_key,
                "LOCAL_SLOCK_SILENT_REPLY: greeting only",
            )
            .await?;
            assert!(
                maybe_hide_silent_streaming_reply(
                    &pool,
                    agent_id,
                    run_id,
                    Some(work_item_id),
                    stream_key,
                )
                .await?
            );

            let remaining: i64 =
                sqlx::query_scalar("select count(*)::bigint from messages where id = $1")
                    .bind(message_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(remaining, 0);
            let status: String =
                sqlx::query_scalar("select status from agent_work_items where id = $1")
                    .bind(work_item_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(status, "silent");
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn streaming_reminder_control_line_is_consumed() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "reminder-agent").await?;
            let channel_id = insert_test_channel(&pool, "reminder-control").await?;
            let root_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', 'remind me later', false)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let work_item_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_work_items (
                    agent_id, channel_id, thread_root_id, source_message_id, title, context, status
                )
                values ($1, $2, $3, $3, 'set reminder', 'remind me later', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .bind(root_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, work_item_id, command, status)
                values ($1, $2, 'codex app-server', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .bind(work_item_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let due_at = (Utc::now() + ChronoDuration::minutes(30)).to_rfc3339();
            let event = json!({
                "type": "reminder_create",
                "when": due_at,
                "title": "Check PR",
                "note": "Look at CI"
            });
            let stream_key = "reminder-run:item-1";
            let body = format!("I'll remind you.\nLOCAL_SLOCK_EVENT {event}");
            let message_id = append_streaming_agent_message(
                &pool,
                agent_id,
                channel_id,
                Some(root_id),
                stream_key,
                &body,
            )
            .await?;

            let hidden = consume_streaming_agent_control_lines(
                &pool,
                agent_id,
                run_id,
                Some(work_item_id),
                stream_key,
            )
            .await?;
            assert!(!hidden);

            let visible_body: String =
                sqlx::query_scalar("select body from messages where id = $1")
                    .bind(message_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(visible_body, "I'll remind you.");

            let reminders = load_reminders(&pool).await?;
            assert_eq!(reminders.len(), 1);
            assert_eq!(reminders[0].title, "Check PR");
            assert_eq!(reminders[0].note, "Look at CI");
            assert_eq!(reminders[0].creator_agent_id, Some(agent_id));
            assert_eq!(reminders[0].channel_id, Some(channel_id));
            assert_eq!(reminders[0].thread_root_id, Some(root_id));
            assert_eq!(reminders[0].message_id, Some(root_id));
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn dm_open_is_idempotent_and_cascades_with_agent() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "dm-idempotent").await?;
            let dm1 = open_dm_with_agent_in_pool(&pool, agent_id).await?;
            let dm2 = open_dm_with_agent_in_pool(&pool, agent_id).await?;
            assert_eq!(dm1, dm2);

            let dm_channel_id = Uuid::parse_str(&dm1).map_err(|err| err.to_string())?;
            let row = sqlx::query(
                r#"
                select c.kind, c.dm_agent_id, count(m.agent_id)::bigint as members
                from channels c
                left join channel_members m on m.channel_id = c.id
                where c.id = $1
                group by c.kind, c.dm_agent_id
                "#,
            )
            .bind(dm_channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let kind: String = row.get("kind");
            let dm_agent_id: Uuid = row.get("dm_agent_id");
            let members: i64 = row.get("members");
            assert_eq!(kind, "dm");
            assert_eq!(dm_agent_id, agent_id);
            assert_eq!(members, 1);

            sqlx::query("delete from agents where id = $1")
                .bind(agent_id)
                .execute(&pool)
                .await
                .map_err(|err| err.to_string())?;
            let remaining: i64 =
                sqlx::query_scalar("select count(*)::bigint from channels where id = $1")
                    .bind(dm_channel_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(remaining, 0);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn delete_agent_cascades_dm_and_preserves_sender_text() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "delete-me").await?;
            let channel_id = insert_test_channel(&pool, "delete-agent").await?;
            let dm_channel_id =
                Uuid::parse_str(&open_dm_with_agent_in_pool(&pool, agent_id).await?)
                    .map_err(|err| err.to_string())?;
            insert_agent_message(&pool, agent_id, dm_channel_id, None, "before delete", false)
                .await?;
            let channel_message_id =
                insert_agent_message(&pool, agent_id, channel_id, None, "channel message", false)
                    .await?;
            delete_agent_in_pool(&pool, agent_id).await?;

            let agent_count: i64 =
                sqlx::query_scalar("select count(*)::bigint from agents where id = $1")
                    .bind(agent_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(agent_count, 0);

            let dm_count: i64 =
                sqlx::query_scalar("select count(*)::bigint from channels where id = $1")
                    .bind(dm_channel_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(dm_count, 0);

            let deleted_activity: i64 = sqlx::query_scalar(
                "select count(*)::bigint from agent_activities where agent_handle = 'delete-me' and title = 'Agent profile deleted'",
            )
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(deleted_activity, 1);

            let message = sqlx::query(
                "select sender_agent_id, sender_name, body from messages where id = $1",
            )
            .bind(channel_message_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(message.get::<Option<Uuid>, _>("sender_agent_id"), None);
            assert_eq!(message.get::<String, _>("sender_name"), "delete-me");
            assert_eq!(message.get::<String, _>("body"), "channel message");
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn delete_channel_removes_timeline_and_unlinks_requests() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "channel-delete-agent").await?;
            let channel_id = insert_test_channel(&pool, "channel-delete").await?;
            sqlx::query("insert into channel_members (channel_id, agent_id) values ($1, $2)")
                .bind(channel_id)
                .bind(agent_id)
                .execute(&pool)
                .await
                .map_err(|err| err.to_string())?;
            let message_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', 'delete channel body', true)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            sqlx::query(
                r#"
                insert into tasks (message_id, channel_id, title, status, assignee_agent_id)
                values ($1, $2, 'delete channel task', 'in_progress', $3)
                "#,
            )
            .bind(message_id)
            .bind(channel_id)
            .bind(agent_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;
            sqlx::query(
                r#"
                insert into reminders (channel_id, creator_agent_id, title, due_at)
                values ($1, $2, 'channel reminder', now() + interval '1 hour')
                "#,
            )
            .bind(channel_id)
            .bind(agent_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let work_item_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_work_items (agent_id, channel_id, source_message_id, title, context, status)
                values ($1, $2, $3, 'request', 'context', 'queued')
                returning id
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .bind(message_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            delete_channel_in_pool(&pool, channel_id).await?;

            let channel_count: i64 =
                sqlx::query_scalar("select count(*)::bigint from channels where id = $1")
                    .bind(channel_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(channel_count, 0);
            let message_count: i64 =
                sqlx::query_scalar("select count(*)::bigint from messages where id = $1")
                    .bind(message_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(message_count, 0);
            let unlinked_work_item_channel: Option<Uuid> =
                sqlx::query_scalar("select channel_id from agent_work_items where id = $1")
                    .bind(work_item_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(unlinked_work_item_channel, None);
            let reminder_channel_count: i64 = sqlx::query_scalar(
                "select count(*)::bigint from reminders where title = 'channel reminder' and channel_id is null",
            )
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(reminder_channel_count, 1);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn dm_rejects_tasks_and_auto_dispatches_owner_messages() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "dm-dispatch").await?;
            let dm_channel_id =
                Uuid::parse_str(&open_dm_with_agent_in_pool(&pool, agent_id).await?)
                    .map_err(|err| err.to_string())?;

            let owner_task_err =
                send_owner_message_in_pool(&pool, dm_channel_id, None, "task body", true, vec![])
                    .await
                    .unwrap_err();
            assert!(owner_task_err.contains("direct messages do not support tasks"));

            let agent_task_err =
                insert_agent_message(&pool, agent_id, dm_channel_id, None, "task body", true)
                    .await
                    .unwrap_err();
            assert!(agent_task_err.contains("direct messages do not support tasks"));

            send_owner_message_in_pool(
                &pool,
                dm_channel_id,
                None,
                "please inspect this",
                false,
                vec![],
            )
            .await?;
            let work_items: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from agent_work_items
                where channel_id = $1 and agent_id = $2 and status = 'queued'
                "#,
            )
            .bind(dm_channel_id)
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(work_items, 1);
            let source_kind: String = sqlx::query_scalar(
                "select source_kind from agent_work_items where channel_id = $1 and agent_id = $2",
            )
            .bind(dm_channel_id)
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(source_kind, "dm");
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn owner_thread_followup_dispatches_to_thread_agents_without_mentions() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "thread-agent").await?;
            let channel_id = insert_test_channel(&pool, "thread-followup").await?;
            sqlx::query(
                r#"
                insert into channel_members (channel_id, agent_id)
                values ($1, $2)
                "#,
            )
            .bind(channel_id)
            .bind(agent_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let root_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', '@thread-agent please investigate', false)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            sqlx::query(
                r#"
                insert into agent_work_items (
                    agent_id, channel_id, thread_root_id, source_message_id, title, context, status
                )
                values ($1, $2, $3, $3, 'Initial dispatch', 'Initial dispatch', 'done')
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .bind(root_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            send_owner_message_in_pool(
                &pool,
                channel_id,
                Some(root_id),
                "我补充一下：这个复现只在 thread 里出现",
                false,
                vec![],
            )
            .await?;

            let work_items = sqlx::query(
                r#"
                select source_message_id, source_kind, title, context
                from agent_work_items
                where channel_id = $1 and agent_id = $2 and source_message_id <> $3
                "#,
            )
            .bind(channel_id)
            .bind(agent_id)
            .bind(root_id)
            .fetch_all(&pool)
            .await
            .map_err(|err| err.to_string())?;

            assert_eq!(work_items.len(), 1);
            assert_eq!(
                work_items[0].get::<String, _>("source_kind"),
                "thread_followup"
            );
            assert_eq!(
                work_items[0]
                    .get::<Option<String>, _>("title")
                    .unwrap_or_default(),
                "我补充一下：这个复现只在 thread 里出现"
            );
            let context: String = work_items[0].get("context");
            assert!(context.contains("Latest thread reply:"));
            assert!(context.contains("Thread reply message id:"));
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn owner_thread_followup_uses_agent_thread_subscription_after_work_done() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "sub-agent").await?;
            let channel_id = insert_test_channel(&pool, "thread-subscription").await?;
            sqlx::query(
                r#"
                insert into channel_members (channel_id, agent_id)
                values ($1, $2)
                "#,
            )
            .bind(channel_id)
            .bind(agent_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let root_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', 'root request', false)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            upsert_agent_thread_subscription(
                &pool,
                agent_id,
                channel_id,
                root_id,
                "mention",
                Some(root_id),
            )
            .await?;

            send_owner_message_in_pool(
                &pool,
                channel_id,
                Some(root_id),
                "继续补充，不需要重新 @ agent",
                false,
                vec![],
            )
            .await?;

            let row = sqlx::query(
                r#"
                select source_kind, thread_root_id, task_id
                from agent_work_items
                where agent_id = $1 and source_message_id <> $2
                order by created_at desc
                limit 1
                "#,
            )
            .bind(agent_id)
            .bind(root_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(row.get::<String, _>("source_kind"), "thread_followup");
            assert_eq!(row.get::<Option<Uuid>, _>("thread_root_id"), Some(root_id));
            assert_eq!(row.get::<Option<Uuid>, _>("task_id"), None);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn mentions_create_agent_requests_but_only_task_mode_creates_global_tasks() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "task-agent").await?;
            let channel_id = insert_test_channel(&pool, "task-semantics").await?;

            send_owner_message_in_pool(
                &pool,
                channel_id,
                None,
                "@task-agent please look at this",
                false,
                vec![],
            )
            .await?;
            let task_count: i64 = sqlx::query_scalar("select count(*)::bigint from tasks")
                .fetch_one(&pool)
                .await
                .map_err(|err| err.to_string())?;
            assert_eq!(task_count, 0);
            let mention_source_kind: String = sqlx::query_scalar(
                "select source_kind from agent_work_items where agent_id = $1 order by created_at desc limit 1",
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(mention_source_kind, "mention");

            send_owner_message_in_pool(
                &pool,
                channel_id,
                None,
                "@task-agent implement the tracked feature",
                true,
                vec![],
            )
            .await?;
            let task_count: i64 = sqlx::query_scalar("select count(*)::bigint from tasks")
                .fetch_one(&pool)
                .await
                .map_err(|err| err.to_string())?;
            assert_eq!(task_count, 1);
            let task_source_kind: String = sqlx::query_scalar(
                "select source_kind from agent_work_items where agent_id = $1 order by created_at desc limit 1",
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(task_source_kind, "task");
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn task_create_event_creates_root_task_and_execution_thread() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "task-thread-agent").await?;
            let channel_id = insert_test_channel(&pool, "task-thread-api").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, working_directory, status)
                values ($1, 'test', '', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::TaskCreate {
                    channel: None,
                    channel_id: Some(channel_id),
                    title: "Investigate task thread API".to_owned(),
                    body: Some("Track this as a durable task".to_owned()),
                    thread_body: Some("Starting execution in the task thread".to_owned()),
                    assign_self: Some(true),
                    status: Some("in_progress".to_owned()),
                },
            )
            .await?;

            let task_row = sqlx::query(
                r#"
                select t.number, t.status, t.assignee_agent_id, m.id as message_id, m.body, m.thread_root_id
                from tasks t
                join messages m on m.id = t.message_id
                where t.channel_id = $1
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let root_message_id: Uuid = task_row.get("message_id");
            assert_eq!(task_row.get::<String, _>("status"), "in_progress");
            assert_eq!(
                task_row.get::<Option<Uuid>, _>("assignee_agent_id"),
                Some(agent_id)
            );
            assert_eq!(task_row.get::<String, _>("body"), "Track this as a durable task");
            assert_eq!(task_row.get::<Option<Uuid>, _>("thread_root_id"), None);

            let reply_body: String = sqlx::query_scalar(
                "select body from messages where channel_id = $1 and thread_root_id = $2",
            )
            .bind(channel_id)
            .bind(root_message_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(reply_body, "Starting execution in the task thread");

            let subscribed: bool = sqlx::query_scalar(
                r#"
                select exists (
                    select 1
                    from agent_thread_subscriptions
                    where agent_id = $1 and channel_id = $2 and thread_root_id = $3
                )
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .bind(root_message_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert!(subscribed);
            assert!(task_row.get::<i64, _>("number") > 0);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn activity_event_records_progress_without_chat_message() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "activity-agent").await?;
            let run_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_runs (agent_id, command, working_directory, status)
                values ($1, 'test', '', 'running')
                returning id
                "#,
            )
            .bind(agent_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            handle_agent_event(
                &pool,
                agent_id,
                run_id,
                AgentEvent::Activity {
                    kind: Some("running_command".to_owned()),
                    title: "Running tests".to_owned(),
                    detail: Some("cargo test".to_owned()),
                },
            )
            .await?;

            let row = sqlx::query(
                r#"
                select kind, phase, status, title, detail
                from agent_activities
                where agent_id = $1 and run_id = $2
                "#,
            )
            .bind(agent_id)
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(row.get::<String, _>("kind"), "command");
            assert_eq!(row.get::<String, _>("phase"), "command");
            assert_eq!(row.get::<String, _>("status"), "active");
            assert_eq!(row.get::<String, _>("title"), "Running tests");
            assert_eq!(row.get::<String, _>("detail"), "cargo test");
            let messages: i64 = sqlx::query_scalar("select count(*)::bigint from messages")
                .fetch_one(&pool)
                .await
                .map_err(|err| err.to_string())?;
            assert_eq!(messages, 0);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn agent_mentions_dispatch_other_agents_once_in_same_thread() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let author_id = insert_test_agent(&pool, "author").await?;
            let reviewer_id = insert_test_agent(&pool, "reviewer").await?;
            let outsider_id = insert_test_agent(&pool, "outsider").await?;
            let channel_id = insert_test_channel(&pool, "collab").await?;
            sqlx::query(
                r#"
                insert into channel_members (channel_id, agent_id)
                values ($1, $2), ($1, $3)
                "#,
            )
            .bind(channel_id)
            .bind(author_id)
            .bind(reviewer_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let message_id = insert_agent_message(
                &pool,
                author_id,
                channel_id,
                None,
                "@author ignore self, @reviewer please review this, @outsider ignore non-member",
                false,
            )
            .await?;
            queue_mentions_as_work_items(
                &pool,
                channel_id,
                None,
                message_id,
                None,
                "@reviewer please review this again",
                MentionDispatchOrigin::Agent {
                    sender_agent_id: author_id,
                },
            )
            .await?;

            let work_items = sqlx::query(
                r#"
                select agent_id, thread_root_id, source_message_id
                from agent_work_items
                where source_message_id = $1
                "#,
            )
            .bind(message_id)
            .fetch_all(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(work_items.len(), 1);
            assert_eq!(work_items[0].get::<Uuid, _>("agent_id"), reviewer_id);
            assert_ne!(work_items[0].get::<Uuid, _>("agent_id"), outsider_id);
            assert_eq!(
                work_items[0].get::<Option<Uuid>, _>("thread_root_id"),
                Some(message_id)
            );
            assert_eq!(
                work_items[0].get::<Option<Uuid>, _>("source_message_id"),
                Some(message_id)
            );
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn agent_mentions_do_not_cross_dispatch_from_dm() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let dm_agent_id = insert_test_agent(&pool, "dm-agent").await?;
            let reviewer_id = insert_test_agent(&pool, "dm-reviewer").await?;
            let dm_channel_id =
                Uuid::parse_str(&open_dm_with_agent_in_pool(&pool, dm_agent_id).await?)
                    .map_err(|err| err.to_string())?;

            insert_agent_message(
                &pool,
                dm_agent_id,
                dm_channel_id,
                None,
                "@dm-reviewer please join this DM",
                false,
            )
            .await?;
            let work_items: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from agent_work_items
                where channel_id = $1 and agent_id = $2
                "#,
            )
            .bind(dm_channel_id)
            .bind(reviewer_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(work_items, 0);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn channel_agent_roster_excludes_current_agent() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let current_id = insert_test_agent(&pool, "current").await?;
            let peer_id = insert_test_agent(&pool, "peer").await?;
            let channel_id = insert_test_channel(&pool, "roster").await?;
            sqlx::query(
                r#"
                insert into channel_members (channel_id, agent_id)
                values ($1, $2), ($1, $3)
                "#,
            )
            .bind(channel_id)
            .bind(current_id)
            .bind(peer_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let roster = load_channel_agent_roster(&pool, Some(channel_id), current_id).await?;
            assert_eq!(roster.len(), 1);
            assert!(roster[0].contains("@peer"));
            assert!(!roster[0].contains("@current"));
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn agent_collaboration_pauses_after_thread_limit() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let author_id = insert_test_agent(&pool, "loop-author").await?;
            let reviewer_id = insert_test_agent(&pool, "loop-reviewer").await?;
            let channel_id = insert_test_channel(&pool, "loop-guard").await?;
            sqlx::query(
                r#"
                insert into channel_members (channel_id, agent_id)
                values ($1, $2), ($1, $3)
                "#,
            )
            .bind(channel_id)
            .bind(author_id)
            .bind(reviewer_id)
            .execute(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let root_id: Uuid = sqlx::query_scalar(
                r#"
                insert into messages (channel_id, sender_name, sender_role, body, is_task)
                values ($1, 'Dylan', 'owner', 'start collaboration', false)
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            for index in 0..10 {
                insert_agent_message(
                    &pool,
                    author_id,
                    channel_id,
                    Some(root_id),
                    &format!("agent loop message {index}"),
                    false,
                )
                .await?;
            }
            insert_agent_message(
                &pool,
                author_id,
                channel_id,
                Some(root_id),
                "@loop-reviewer continue the loop",
                false,
            )
            .await?;

            let work_items: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from agent_work_items
                where channel_id = $1 and agent_id = $2
                "#,
            )
            .bind(channel_id)
            .bind(reviewer_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(work_items, 0);
            let system_messages: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from messages
                where channel_id = $1
                  and thread_root_id = $2
                  and sender_role = 'system'
                  and body like 'Inter-agent collaboration paused:%'
                "#,
            )
            .bind(channel_id)
            .bind(root_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(system_messages, 1);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn due_reminder_fires_system_message() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let channel_id = insert_test_channel(&pool, "reminders").await?;
            let reminder_id: Uuid = sqlx::query_scalar(
                r#"
                insert into reminders (channel_id, title, note, due_at, recurrence, status)
                values ($1, 'Check thread', 'Follow up with Dylan', now() - interval '1 minute', 'none', 'scheduled')
                returning id
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            process_due_reminders(&pool).await?;

            let status: String = sqlx::query_scalar("select status from reminders where id = $1")
                .bind(reminder_id)
                .fetch_one(&pool)
                .await
                .map_err(|err| err.to_string())?;
            assert_eq!(status, "fired");
            let system_messages: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from messages
                where channel_id = $1
                  and sender_role = 'system'
                  and body like 'Reminder:%'
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(system_messages, 1);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn due_agent_schedule_dispatches_work_item() {
        let Some((pool, schema)) = test_pool().await else {
            return;
        };
        let result: Result<(), String> = async {
            let agent_id = insert_test_agent(&pool, "scheduler").await?;
            let channel_id = insert_test_channel(&pool, "scheduled").await?;
            let schedule_id: Uuid = sqlx::query_scalar(
                r#"
                insert into agent_schedules (
                    agent_id, channel_id, title, prompt, cadence, next_run_at, status
                )
                values ($1, $2, 'Daily check', 'Summarize open work', 'daily', now() - interval '1 minute', 'active')
                returning id
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            process_due_agent_schedules(&pool).await?;

            let schedule = sqlx::query(
                "select last_run_at, last_work_item_id, next_run_at > now() as future from agent_schedules where id = $1",
            )
            .bind(schedule_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            let last_run_at: Option<DateTime<Utc>> = schedule.get("last_run_at");
            let last_work_item_id: Option<Uuid> = schedule.get("last_work_item_id");
            let future: bool = schedule.get("future");
            assert!(last_run_at.is_some());
            assert!(last_work_item_id.is_some());
            assert!(future);

            let work_items: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from agent_work_items
                where agent_id = $1
                  and channel_id = $2
                  and title = 'Daily check'
                  and context like '%Scheduled routine id:%'
                "#,
            )
            .bind(agent_id)
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(work_items, 1);

            let system_messages: i64 = sqlx::query_scalar(
                r#"
                select count(*)::bigint
                from messages
                where channel_id = $1
                  and sender_role = 'system'
                  and body like 'Scheduled routine for @scheduler:%'
                "#,
            )
            .bind(channel_id)
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;
            assert_eq!(system_messages, 1);
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }

    #[tokio::test]
    async fn supervisor_command_claim_is_single_consumer() {
        let Some((pool, schema)) = test_pool_with_connections(8).await else {
            return;
        };
        let result: Result<(), String> = async {
            let command_id: Uuid = sqlx::query_scalar(
                r#"
                insert into supervisor_commands (command_type)
                values ('start_agent')
                returning id
                "#,
            )
            .fetch_one(&pool)
            .await
            .map_err(|err| err.to_string())?;

            let (r1, r2, r3, r4) = tokio::join!(
                claim_next_supervisor_command(&pool),
                claim_next_supervisor_command(&pool),
                claim_next_supervisor_command(&pool),
                claim_next_supervisor_command(&pool)
            );
            let mut claimed = Vec::new();
            for result in [r1, r2, r3, r4] {
                if let Some(command) = result? {
                    claimed.push(command.id);
                }
            }

            assert_eq!(claimed, vec![command_id]);
            let status: String =
                sqlx::query_scalar("select status from supervisor_commands where id = $1")
                    .bind(command_id)
                    .fetch_one(&pool)
                    .await
                    .map_err(|err| err.to_string())?;
            assert_eq!(status, "running");
            Ok(())
        }
        .await;
        drop_test_schema(pool, schema).await;
        assert!(result.is_ok(), "{:?}", result.err());
    }
}
