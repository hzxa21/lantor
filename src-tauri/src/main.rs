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
const UI_REFRESH_CHANNEL: &str = "localslock_ui_refresh";
const SUPERVISOR_WAKE_CHANNEL: &str = "localslock_supervisor_wake";
const UI_REFRESH_EVENT: &str = "localslock://refresh";
const STREAMING_MESSAGE_BODY_LIMIT: usize = 200_000;
const STREAMING_TRUNCATION_MARKER: &str = "\n\n[stream truncated by LocalSlock]";
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
    created_at: DateTime<Utc>,
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
    TaskStatus {
        task_number: i64,
        status: String,
    },
    TaskClaim {
        task_number: i64,
        assignee_handle: Option<String>,
    },
}

#[derive(Debug, Serialize)]
struct Bootstrap {
    db_url: String,
    channels: Vec<Channel>,
    channel_members: Vec<ChannelMember>,
    agents: Vec<Agent>,
    messages: Vec<Message>,
    tasks: Vec<Task>,
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
    } else {
        let _ = notify_ui_refresh(pool, reason).await;
    }
}

async fn notify_supervisor_wake(pool: &PgPool) -> CommandResult<()> {
    notify_postgres(pool, SUPERVISOR_WAKE_CHANNEL, "wake").await
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
                    or lower(title) like '%ready%'
                    or lower(title) like '%accepted%' then 'success'
                when lower(title) like '%running%'
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
        "alter table agent_runs add column if not exists work_item_id uuid references agent_work_items(id) on delete set null",
    )
    .execute(pool)
    .await?;

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
    let tasks = load_tasks(&state.pool).await?;
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
        tasks,
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
    sqlx::query("delete from channels where id = $1")
        .bind(channel_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;

    Ok(())
}

#[tauri::command]
async fn open_dm_with_agent(agent_id: Uuid, state: State<'_, AppState>) -> CommandResult<String> {
    open_dm_with_agent_in_pool(&state.pool, agent_id).await
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
    runtime: String,
    model: String,
    launch_command: String,
    working_directory: String,
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
    let avatar = normalized_handle
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "A".to_owned());

    let agent_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agents (
            handle, display_name, role, status, runtime, model, avatar, description,
            launch_command, working_directory
        )
        values ($1, $2, 'agent', 'idle', $3, $4, $5, 'Local agent', $6, $7)
        on conflict (handle) do update set
            display_name = excluded.display_name,
            runtime = excluded.runtime,
            model = excluded.model,
            avatar = excluded.avatar,
            launch_command = excluded.launch_command,
            working_directory = excluded.working_directory
        returning id
        "#,
    )
    .bind(normalized_handle)
    .bind(display_name)
    .bind(runtime.trim())
    .bind(model.trim())
    .bind(avatar)
    .bind(launch_command.trim())
    .bind(working_directory.trim())
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
    runtime: String,
    model: String,
    description: String,
    launch_command: String,
    working_directory: String,
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
    let avatar = normalized_handle
        .chars()
        .next()
        .map(|c| c.to_uppercase().to_string())
        .unwrap_or_else(|| "A".to_owned());

    sqlx::query(
        r#"
        update agents
        set handle = $2,
            display_name = $3,
            runtime = $4,
            model = $5,
            avatar = $6,
            description = $7,
            launch_command = $8,
            working_directory = $9
        where id = $1
        "#,
    )
    .bind(agent_id)
    .bind(normalized_handle)
    .bind(display_name)
    .bind(runtime.trim())
    .bind(model.trim())
    .bind(avatar)
    .bind(description.trim())
    .bind(launch_command.trim())
    .bind(working_directory.trim())
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
        return Err("stop the agent before deleting it".to_owned());
    }

    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "profile",
        "Agent profile deleted",
        "",
    )
    .await?;

    sqlx::query("delete from agents where id = $1")
        .bind(agent_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;

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
                .unwrap_or_else(|| "LocalSlock work item".to_owned())
            }
            None => "LocalSlock work item".to_owned(),
        };
    }

    let work_context = context.trim();
    let work_item_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_work_items (
            agent_id, channel_id, thread_root_id, task_id, title, context, status
        )
        values ($1, $2, $3, $4, $5, $6, 'queued')
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(resolved_channel_id)
    .bind(resolved_thread_root_id)
    .bind(task_id)
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

    let scheduled = enqueue_agent_work_if_available(&state.pool, agent_id, work_item_id).await?;
    record_agent_activity(
        &state.pool,
        Some(agent_id),
        None,
        "dispatch",
        if scheduled {
            "Work item dispatched"
        } else {
            "Work item queued"
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
                return Err("running work item does not have a run id".to_owned());
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
        other => return Err(format!("cannot cancel work item with status {other}")),
    }

    record_agent_activity(
        &state.pool,
        Some(agent_id),
        run_id,
        "dispatch",
        "Work item cancel requested",
        work_item_id.to_string(),
    )
    .await?;

    Ok(())
}

#[tauri::command]
async fn retry_agent_work(work_item_id: Uuid, state: State<'_, AppState>) -> CommandResult<Uuid> {
    let row = sqlx::query(
        r#"
        select agent_id, channel_id, thread_root_id, source_message_id, task_id, title, context, status
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
        return Err(format!("cannot retry work item with status {old_status}"));
    }

    let agent_id: Uuid = row.get("agent_id");
    let title: String = row.get("title");
    let context: String = row.get("context");
    let new_work_item_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_work_items (
            agent_id, channel_id, thread_root_id, source_message_id, task_id, title, context, status
        )
        values ($1, $2, $3, $4, $5, $6, $7, 'queued')
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(row.get::<Option<Uuid>, _>("channel_id"))
    .bind(row.get::<Option<Uuid>, _>("thread_root_id"))
    .bind(row.get::<Option<Uuid>, _>("source_message_id"))
    .bind(row.get::<Option<Uuid>, _>("task_id"))
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
            "Work item retried"
        } else {
            "Retried work item queued"
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

async fn queue_mentions_as_work_items(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    message_id: Uuid,
    task_id: Option<Uuid>,
    body: &str,
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
    if channel_kind == "dm" {
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
            targets.push((agent_id, handle));
        }
    }

    if targets.is_empty() {
        return Ok(());
    }
    let reply_thread_root_id = thread_root_id.unwrap_or(message_id);

    let title = body
        .lines()
        .next()
        .map(|line| line.chars().take(120).collect::<String>())
        .filter(|line| !line.trim().is_empty())
        .unwrap_or_else(|| format!("Mention in #{channel_name}"));

    for (agent_id, agent_handle) in targets {
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
        let context = build_mention_work_context(
            pool,
            channel_id,
            &channel_label,
            Some(reply_thread_root_id),
            message_id,
            body,
        )
        .await?;
        let work_item_id: Uuid = sqlx::query_scalar(
            r#"
            insert into agent_work_items (
                agent_id, channel_id, thread_root_id, source_message_id, task_id, title, context, status
            )
            values ($1, $2, $3, $4, $5, $6, $7, 'queued')
            returning id
            "#,
        )
        .bind(agent_id)
        .bind(channel_id)
        .bind(reply_thread_root_id)
        .bind(message_id)
        .bind(task_id)
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
            if channel_kind == "dm" {
                "dm"
            } else {
                "mention"
            },
            if scheduled {
                if channel_kind == "dm" {
                    "DM dispatched"
                } else {
                    "Mention dispatched"
                }
            } else {
                if channel_kind == "dm" {
                    "DM queued"
                } else {
                    "Mention queued"
                }
            },
            format!("#{channel_name} to @{agent_handle}: {title}"),
        )
        .await?;
    }

    Ok(())
}

async fn build_mention_work_context(
    pool: &PgPool,
    channel_id: Uuid,
    channel_label: &str,
    thread_root_id: Option<Uuid>,
    message_id: Uuid,
    body: &str,
) -> CommandResult<String> {
    let mut lines = vec![
        format!("Surface: {channel_label}"),
        format!("Mentioned message id: {message_id}"),
    ];
    if let Some(thread_root_id) = thread_root_id {
        lines.push(format!("Thread root id: {thread_root_id}"));
    }
    lines.push("Mentioned message from Dylan:".to_owned());
    lines.push(body.trim().to_owned());

    if let Some(thread_root_id) = thread_root_id {
        let rows = sqlx::query(
            r#"
            select sender_name, sender_role, body, created_at
            from messages
            where id = $1 or thread_root_id = $1
            order by created_at desc
            limit 12
            "#,
        )
        .bind(thread_root_id)
        .fetch_all(pool)
        .await
        .map_err(to_string)?;

        if !rows.is_empty() {
            lines.push("Recent thread context, oldest first:".to_owned());
            for row in rows.into_iter().rev() {
                let sender_name: String = row.get("sender_name");
                let sender_role: String = row.get("sender_role");
                let created_at: DateTime<Utc> = row.get("created_at");
                let body: String = row.get("body");
                lines.push(format!(
                    "- {sender_name} ({sender_role}) at {}: {}",
                    created_at.to_rfc3339(),
                    body.replace('\n', "\n  ")
                ));
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

    Ok(lines.join("\n\n"))
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
    state: State<'_, AppState>,
) -> CommandResult<()> {
    send_owner_message_in_pool(&state.pool, channel_id, thread_root_id, &body, as_task).await
}

async fn send_owner_message_in_pool(
    pool: &PgPool,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    body: &str,
    as_task: bool,
) -> CommandResult<()> {
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
    let result = sqlx::query("update messages set body = $2 where id = $1")
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
    sqlx::query(
        r#"
        update tasks
        set assignee_agent_id = $2,
            status = case when $2 is null then status else 'in_progress' end,
            updated_at = now()
        where id = $1
        "#,
    )
    .bind(task_id)
    .bind(agent_id)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

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

    sqlx::query(
        r#"
        update tasks
        set status = $2, updated_at = now()
        where id = $1
        "#,
    )
    .bind(task_id)
    .bind(status)
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

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

    sqlx::query("update messages set body = $2 where id = $1")
        .bind(message_id)
        .bind(title)
        .execute(&mut *tx)
        .await
        .map_err(to_string)?;

    tx.commit().await.map_err(to_string)?;
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
            working_directory
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
            m.created_at
        from messages m
        left join tasks t on t.message_id = m.id
        order by m.created_at asc
        "#,
    )
    .fetch_all(pool)
    .await
    .map_err(to_string)?;

    Ok(rows
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
            created_at: row.get("created_at"),
        })
        .collect())
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
            m.created_at
        from messages m
        left join tasks t on t.message_id = m.id
        where m.id = $1
        "#,
    )
    .bind(message_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    Ok(Message {
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
        created_at: row.get("created_at"),
    })
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
        "tools" => "tools",
        "error" | "event_error" | "run_error" => "error",
        "run" => "runtime",
        "dispatch" | "mention" | "dm" | "task" => "work",
        "profile" => "profile",
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
        || lowered.contains("ready")
        || lowered.contains("accepted")
    {
        "success"
    } else if lowered.contains("running")
        || lowered.contains("started")
        || lowered.contains("queued")
        || lowered.contains("dispatched")
        || lowered.contains("thinking")
        || lowered.contains("using")
    {
        "active"
    } else {
        "info"
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
                    let _ = record_agent_activity(
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
    handle_agent_event(pool, agent_id, event).await.map(Some)
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
                    handle_agent_event(pool, agent_id, event).await
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
        AgentEvent::TaskStatus {
            task_number,
            status,
        } => {
            let status = status.trim();
            if !matches!(status, "todo" | "in_progress" | "in_review" | "done") {
                return Err(format!("unsupported task status: {status}"));
            }
            let affected = sqlx::query(
                r#"
                update tasks
                set status = $2, updated_at = now()
                where number = $1
                "#,
            )
            .bind(task_number)
            .bind(status)
            .execute(pool)
            .await
            .map_err(to_string)?
            .rows_affected();
            if affected == 0 {
                return Err(format!("task #{task_number} does not exist"));
            }
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
            let affected = sqlx::query(
                r#"
                update tasks
                set assignee_agent_id = $2,
                    status = case when $2 is null then status else 'in_progress' end,
                    updated_at = now()
                where number = $1
                "#,
            )
            .bind(task_number)
            .bind(assignee)
            .execute(pool)
            .await
            .map_err(to_string)?
            .rows_affected();
            if affected == 0 {
                return Err(format!("task #{task_number} does not exist"));
            }
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
    }
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
    let _ = notify_ui_refresh(pool, "message").await;
    Ok(msg_id)
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
            .bind(append_delta)
            .bind(if truncated { "complete" } else { "streaming" })
            .execute(pool)
            .await
            .map_err(to_string)?;
        if let Ok(message) = load_message(pool, message_id).await {
            let _ = notify_ui_message_upsert(pool, &message, "stream_delta").await;
        } else {
            let _ = notify_ui_refresh(pool, "stream_delta").await;
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
        }
    }
    Ok(())
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
            format!("Work item {work_status}"),
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

fn build_work_item_prompt(
    work_item_id: Uuid,
    title: &str,
    context: &str,
    channel_name: Option<&str>,
    task_number: Option<i64>,
    thread_root_id: Option<Uuid>,
) -> String {
    let mut lines = vec![
        "Current LocalSlock work item:".to_owned(),
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
    if !context.trim().is_empty() {
        lines.push("context:".to_owned());
        lines.push(context.trim().to_owned());
    }
    lines.push(
        "When you finish, write results back with LOCAL_SLOCK_EVENT lines and update task status if applicable."
            .to_owned(),
    );
    lines.join("\n")
}

fn build_codex_streaming_prompt(legacy_prompt: &str) -> String {
    if legacy_prompt.trim().is_empty() {
        return "No current LocalSlock work item is assigned. Reply with a short ready status."
            .to_owned();
    }
    legacy_prompt.replace(
        "When you finish, write results back with LOCAL_SLOCK_EVENT lines and update task status if applicable.",
        "Reply normally. LocalSlock will stream your assistant text into the correct channel/thread automatically. Do not emit LOCAL_SLOCK_EVENT lines in this Codex JSON streaming mode.",
    )
}

fn codex_developer_instructions(handle: &str) -> String {
    format!(
        "You are @{handle}, a local agent running inside LocalSlock.\n\
         You collaborate with one local human through channels, threads, tasks, and DMs.\n\
         LocalSlock is connected to Codex through the official app-server JSON protocol and streams your assistant text into chat automatically.\n\
         Do not print LOCAL_SLOCK_EVENT lines unless explicitly asked to debug the legacy runtime path.\n\
         Keep user-visible replies concise and include concrete results or blockers."
    )
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

fn first_nonempty_item_string<'a>(item: &'a Value, fields: &[&str]) -> Option<&'a str> {
    fields
        .iter()
        .find_map(|field| item.get(*field).and_then(Value::as_str))
        .filter(|value| !value.trim().is_empty())
}

fn codex_tool_completion_summary(value: &Value) -> Option<String> {
    let item = value.pointer("/params/item")?;
    let item_type = item.get("type").and_then(Value::as_str).unwrap_or("item");
    if !matches!(
        item_type,
        "commandExecution" | "mcpToolCall" | "dynamicToolCall" | "webSearch" | "fileChange"
    ) {
        return None;
    }

    let mut detail = codex_item_summary(value);
    if let Some(exit_code) = item.get("exitCode").and_then(Value::as_i64) {
        detail.push_str(&format!(" · exit {exit_code}"));
    }
    if let Some(status) = first_nonempty_item_string(item, &["status", "state"]) {
        detail.push_str(&format!(" · {status}"));
    }
    if let Some(output) =
        first_nonempty_item_string(item, &["output", "stdout", "stderr", "result", "error"])
    {
        detail.push_str("\n");
        detail.push_str(output);
    }

    Some(truncate_activity_detail(&detail))
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
        return "No current LocalSlock work item is assigned. Reply with a short ready status."
            .to_owned();
    }
    legacy_prompt.replace(
        "When you finish, write results back with LOCAL_SLOCK_EVENT lines and update task status if applicable.",
        "Reply normally. LocalSlock will stream your assistant text into the correct channel/thread automatically. Do not emit LOCAL_SLOCK_EVENT lines in this Claude stream-json mode.",
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
            Some("init") => Some((
                "run",
                "Claude stream initialized",
                "session ready".to_owned(),
            )),
            Some("api_retry") => Some((
                "run_error",
                "Claude API retry",
                truncate_activity_detail(&value.to_string()),
            )),
            Some(subtype) => Some(("activity", "Claude system event", subtype.to_owned())),
            None => value
                .get("status")
                .and_then(Value::as_str)
                .map(|status| ("activity", "Claude status", status.to_owned())),
        },
        "rate_limit_event" => {
            let status = value
                .pointer("/rate_limit_info/status")
                .and_then(Value::as_str)
                .unwrap_or("unknown");
            let kind = if status.eq_ignore_ascii_case("allowed") {
                "run"
            } else {
                "run_error"
            };
            Some((
                kind,
                "Claude rate limit status",
                truncate_activity_detail(&value.to_string()),
            ))
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
                        Some(("tools", "Using tools", name.to_owned()))
                    } else if block_type == "thinking" {
                        Some(("thinking", "Thinking", "Claude is thinking".to_owned()))
                    } else {
                        None
                    }
                }
                "content_block_stop" => Some((
                    "acting",
                    "Claude content block finished",
                    event_type.to_owned(),
                )),
                "message_stop" => Some(("run", "Claude message finished", event_type.to_owned())),
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
) -> CommandResult<Arc<WarmClaudeRuntime>> {
    let model = if model.trim().is_empty() {
        "sonnet".to_owned()
    } else {
        model.trim().to_owned()
    };
    let mut command = Command::new("claude");
    command
        .arg("-p")
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
) -> CommandResult<Arc<WarmCodexRuntime>> {
    let cwd = effective_codex_cwd(working_directory)?;
    let mut command = Command::new("/bin/zsh");
    command
        .arg("-lc")
        .arg("exec codex app-server --listen stdio://");
    command.env("LOCAL_SLOCK_AGENT_ID", agent_id.to_string());
    command.env("LOCAL_SLOCK_AGENT_HANDLE", handle);
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
    let developer_instructions = codex_developer_instructions(handle);
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

    loop {
        write_supervisor_heartbeat(&pool).await?;
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
                "Backlog work item scheduled",
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
        "Codex turn interrupt requested",
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
        "Follow-up steered into active turn",
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
                "Codex runtime busy",
                work_item_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "no work item".to_owned()),
            )
            .await?;
            return Ok(());
        }
    }

    let initial_log = if codex_prompt.is_empty() {
        format!("$ {command_text}\n[warm process reused]\n")
    } else {
        format!(
            "$ {command_text}\n[warm process reused]\n\n[streaming work item]\n{codex_prompt}\n"
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
            "Work item running",
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
        "Codex warm turn started",
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
                "Claude runtime busy",
                work_item_id
                    .map(|id| id.to_string())
                    .unwrap_or_else(|| "no work item".to_owned()),
            )
            .await?;
            return Ok(());
        }
    }

    let initial_log = if claude_prompt.is_empty() {
        format!("$ {command_text}\n[warm process reused]\n")
    } else {
        format!(
            "$ {command_text}\n[warm process reused]\n\n[streaming work item]\n{claude_prompt}\n"
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
            "Work item running",
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
        "Claude warm turn started",
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
                "Cancelled work item skipped",
                work_item_id.to_string(),
            )
            .await?;
            return Ok(());
        }
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
    let work_item_prompt = match work_item_id {
        Some(work_item_id) => {
            let row = sqlx::query(
                r#"
                select
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
            let channel_name: Option<String> = row.get("channel_name");
            let task_number: Option<i64> = row.get("task_number");
            let thread_root_id: Option<Uuid> = row.get("thread_root_id");
            build_work_item_prompt(
                work_item_id,
                &title,
                &context,
                channel_name.as_deref(),
                task_number,
                thread_root_id,
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
        )
        .await;
    }
    let command_text = effective_launch_command(launch_command, runtime, model, handle.clone());
    let initial_log = if work_item_prompt.is_empty() {
        format!("$ {command_text}\n")
    } else {
        format!("$ {command_text}\n\n[work item]\n{work_item_prompt}\n")
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
            "Work item running",
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
            "Follow-up steer accepted"
        } else {
            "Follow-up steer rejected; queued"
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
                "Claude first token",
                format!("{} ms after user_message", elapsed.as_millis()),
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
                        "Codex turn accepted",
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
                        "Codex turn interrupt accepted",
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
                    "Codex first token",
                    format!("{} ms after turn/start", elapsed.as_millis()),
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
                    stream_key,
                )
            };
            if let Some(channel_id) = active.1 {
                let existing: Option<Uuid> =
                    sqlx::query_scalar("select id from messages where stream_key = $1")
                        .bind(&active.3)
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
                            pool, agent_id, channel_id, active.2, &active.3, text,
                        )
                        .await?;
                    }
                }
                finish_streaming_agent_message(pool, &active.3, "complete").await?;
            }
        }
        Some("item/completed") => {
            let Some(run_id) = active_run_id else {
                return Ok(());
            };
            if let Some(detail) = codex_tool_completion_summary(&value) {
                record_agent_activity(
                    pool,
                    Some(agent_id),
                    Some(run_id),
                    "tools",
                    "Tool completed",
                    detail,
                )
                .await?;
            }
        }
        Some("item/started") => {
            let Some(run_id) = active_run_id else {
                return Ok(());
            };
            let item_type = codex_item_type(&value).unwrap_or("item");
            let (kind, title) = match item_type {
                "reasoning" => ("thinking", "Thinking"),
                "commandExecution" | "mcpToolCall" | "dynamicToolCall" | "webSearch"
                | "fileChange" => ("tools", "Using tools"),
                "agentMessage" => ("acting", "Writing response"),
                _ => ("activity", "Codex activity"),
            };
            record_agent_activity_throttled(
                pool,
                Some(agent_id),
                Some(run_id),
                kind,
                title,
                codex_item_summary(&value),
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
        let work_status = if was_cancelled {
            "cancelled"
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
            format!("Work item {work_status}"),
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
            "Claude warm turn cancelled"
        } else if success {
            "Claude warm turn completed"
        } else {
            "Claude warm turn failed"
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
        let work_status = if was_cancelled {
            "cancelled"
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
            format!("Work item {work_status}"),
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
            "Codex warm turn cancelled"
        } else if success {
            "Codex warm turn completed"
        } else {
            "Codex warm turn failed"
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
                    "Stop signal sent",
                    "Codex turn/interrupt sent to warm runtime",
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
        "Stop signal sent",
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

    tauri::Builder::default()
        .manage(AppState {
            pool,
            db_url: state_db_url,
        })
        .setup(move |app| {
            spawn_ui_refresh_listener(app.handle().clone(), database_url.clone());
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("LocalSlock");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            cancel_agent_work,
            check_runtime,
            create_agent,
            create_channel,
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
            start_agent,
            stop_agent,
            uninstall_supervisor_service,
            update_agent,
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
    if env::args().any(|arg| arg == "--supervisor") {
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
        append_streaming_agent_message, capped_stream_delta, claim_next_supervisor_command,
        claude_message_text, claude_result_error, claude_text_delta, codex_turn_id_from_value,
        extract_agent_event_json, extract_agent_mentions, finish_streaming_agent_message,
        insert_agent_message, load_messages, load_runtime_thread_id, migrate,
        open_dm_with_agent_in_pool, parse_activity_metadata, send_owner_message_in_pool,
        upsert_runtime_thread_id, DEFAULT_DATABASE_URL, STREAMING_MESSAGE_BODY_LIMIT,
        STREAMING_TRUNCATION_MARKER,
    };
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
    }

    async fn test_pool() -> Option<(PgPool, String)> {
        test_pool_with_connections(1).await
    }

    async fn test_pool_with_connections(max_connections: u32) -> Option<(PgPool, String)> {
        let database_url = std::env::var("LOCAL_SLOCK_TEST_DATABASE_URL")
            .unwrap_or_else(|_| DEFAULT_DATABASE_URL.to_owned());
        let pool = match PgPoolOptions::new()
            .max_connections(max_connections)
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
            .execute(&pool)
            .await
        {
            eprintln!("skipping postgres-backed LocalSlock DM test: {err}");
            pool.close().await;
            return None;
        }
        if let Err(err) = sqlx::query(&format!(r#"set search_path to "{schema}", public"#))
            .execute(&pool)
            .await
        {
            eprintln!("skipping postgres-backed LocalSlock DM test: {err}");
            let _ = sqlx::query(&format!(r#"drop schema if exists "{schema}" cascade"#))
                .execute(&pool)
                .await;
            pool.close().await;
            return None;
        }
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
                send_owner_message_in_pool(&pool, dm_channel_id, None, "task body", true)
                    .await
                    .unwrap_err();
            assert!(owner_task_err.contains("direct messages do not support tasks"));

            let agent_task_err =
                insert_agent_message(&pool, agent_id, dm_channel_id, None, "task body", true)
                    .await
                    .unwrap_err();
            assert!(agent_task_err.contains("direct messages do not support tasks"));

            send_owner_message_in_pool(&pool, dm_channel_id, None, "please inspect this", false)
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
