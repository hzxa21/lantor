#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    collections::HashSet,
    env, fs,
    path::PathBuf,
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use tauri::{Manager, State};
use tokio::{
    io::{AsyncBufReadExt, AsyncRead, BufReader},
    process::Command,
    time::sleep,
};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://dylan:123456@127.0.0.1:5432/localslock";
const SUPERVISOR_LOCK_ID: i64 = 2_026_050_101;
const LAUNCH_AGENT_LABEL: &str = "local.localslock.supervisor";
const AGENT_EVENT_PREFIX: &str = "LOCAL_SLOCK_EVENT ";

#[derive(Clone)]
struct AppState {
    pool: PgPool,
    db_url: String,
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
struct AgentActivity {
    id: Uuid,
    agent_id: Option<Uuid>,
    agent_handle: String,
    run_id: Option<Uuid>,
    kind: String,
    title: String,
    detail: String,
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
            title text not null,
            detail text not null default '',
            created_at timestamptz not null default now()
        )
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
async fn create_channel(name: String, state: State<'_, AppState>) -> CommandResult<()> {
    let normalized = normalize_channel_name(&name);
    if normalized.is_empty() {
        return Err("channel name is empty".to_owned());
    }

    sqlx::query(
        r#"
        insert into channels (name, description)
        values ($1, 'Local channel')
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
    let channel_name: Option<String> =
        sqlx::query_scalar("select name from channels where id = $1")
            .bind(channel_id)
            .fetch_optional(&state.pool)
            .await
            .map_err(to_string)?;
    let Some(channel_name) = channel_name else {
        return Err("channel does not exist".to_owned());
    };

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

async fn agent_has_active_or_pending_start(pool: &PgPool, agent_id: Uuid) -> CommandResult<bool> {
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
    if agent_has_active_or_pending_start(pool, agent_id).await? {
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
    sqlx::query("update agents set status = 'queued' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;

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
    if mentions.is_empty() {
        return Ok(());
    }
    let reply_thread_root_id = thread_root_id.unwrap_or(message_id);

    let channel_name: String = sqlx::query_scalar("select name from channels where id = $1")
        .bind(channel_id)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let title = body
        .lines()
        .next()
        .map(|line| line.chars().take(120).collect::<String>())
        .filter(|line| !line.trim().is_empty())
        .unwrap_or_else(|| format!("Mention in #{channel_name}"));

    for handle in mentions {
        let agent_id: Option<Uuid> = sqlx::query_scalar("select id from agents where handle = $1")
            .bind(&handle)
            .fetch_optional(pool)
            .await
            .map_err(to_string)?;
        let Some(agent_id) = agent_id else {
            continue;
        };

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

        let context = build_mention_work_context(
            pool,
            channel_id,
            &channel_name,
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
        let scheduled = enqueue_agent_work_if_available(pool, agent_id, work_item_id).await?;
        record_agent_activity(
            pool,
            Some(agent_id),
            None,
            "mention",
            if scheduled {
                "Mention dispatched"
            } else {
                "Mention queued"
            },
            format!("#{channel_name}: {title}"),
        )
        .await?;
    }

    Ok(())
}

async fn build_mention_work_context(
    pool: &PgPool,
    channel_id: Uuid,
    channel_name: &str,
    thread_root_id: Option<Uuid>,
    message_id: Uuid,
    body: &str,
) -> CommandResult<String> {
    let mut lines = vec![
        format!("Channel: #{channel_name}"),
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
    lines.push("Use normal stdout for private logs; only LOCAL_SLOCK_EVENT creates visible chat messages.".to_owned());

    Ok(lines.join("\n\n"))
}

#[tauri::command]
async fn stop_agent(run_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    let row = sqlx::query(
        r#"
        select agent_id, pid
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
    sqlx::query("update agent_runs set status = 'stopping' where id = $1")
        .bind(run_id)
        .execute(&state.pool)
        .await
        .map_err(to_string)?;
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
    let mut tx = state.pool.begin().await.map_err(to_string)?;
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
        &state.pool,
        channel_id,
        thread_root_id,
        msg_id,
        task_id,
        body.trim(),
    )
    .await?;
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
            count(m.id) filter (
                where m.created_at > coalesce(r.last_read_at, '-infinity'::timestamptz)
            )::integer as unread_count
        from channels c
        left join channel_read_state r on r.channel_id = c.id
        left join messages m on m.channel_id = c.id
        group by c.id, c.name, c.description
        order by case when c.name = 'local-slock' then 0 else 1 end, c.name
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
            task_number: row.get("task_number"),
            task_status: row.get("task_status"),
            created_at: row.get("created_at"),
        })
        .collect())
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

async fn load_agent_activities(pool: &PgPool) -> CommandResult<Vec<AgentActivity>> {
    let rows = sqlx::query(
        r#"
        select id, agent_id, agent_handle, run_id, kind, title, detail, created_at
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
            title: row.get("title"),
            detail: row.get("detail"),
            created_at: row.get("created_at"),
        })
        .collect())
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

    sqlx::query(
        r#"
        insert into agent_activities (agent_id, agent_handle, run_id, kind, title, detail)
        values ($1, $2, $3, $4, $5, $6)
        "#,
    )
    .bind(agent_id)
    .bind(agent_handle)
    .bind(run_id)
    .bind(kind)
    .bind(title.as_ref())
    .bind(detail.as_ref())
    .execute(pool)
    .await
    .map_err(to_string)?;

    Ok(())
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
    Ok(msg_id)
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
    let (run_status, agent_status, exit_code, log_line) = match result {
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

    loop {
        write_supervisor_heartbeat(&pool).await?;
        schedule_queued_work_items(&pool).await?;
        if let Some(command) = claim_next_supervisor_command(&pool).await? {
            let command_id = command.id;
            let result = process_supervisor_command(&pool, command).await;
            finish_supervisor_command(&pool, command_id, result.err()).await?;
        }
        sleep(Duration::from_millis(800)).await;
    }
}

async fn schedule_queued_work_items(pool: &PgPool) -> CommandResult<()> {
    let rows = sqlx::query(
        r#"
        select w.id, w.agent_id
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
        select id, command_type, agent_id, run_id, work_item_id
        from supervisor_commands
        where status = 'pending'
        order by created_at asc
        limit 1
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

    sqlx::query(
        "update supervisor_commands set status = 'running', updated_at = now() where id = $1",
    )
    .bind(command.id)
    .execute(pool)
    .await
    .map_err(to_string)?;

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
    command: SupervisorCommand,
) -> CommandResult<()> {
    match command.command_type.as_str() {
        "start_agent" => {
            let Some(agent_id) = command.agent_id else {
                return Err("start_agent command missing agent_id".to_owned());
            };
            supervisor_start_agent(pool, agent_id, command.work_item_id).await
        }
        "stop_run" => {
            let Some(run_id) = command.run_id else {
                return Err("stop_run command missing run_id".to_owned());
            };
            supervisor_stop_run(pool, run_id).await
        }
        other => Err(format!("unknown supervisor command: {other}")),
    }
}

async fn supervisor_start_agent(
    pool: &PgPool,
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
    let command_text = effective_launch_command(launch_command, runtime, model, handle.clone());
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

async fn supervisor_stop_run(pool: &PgPool, run_id: Uuid) -> CommandResult<()> {
    let row = sqlx::query(
        r#"
        select agent_id, pid
        from agent_runs
        where id = $1 and stopped_at is null
        "#,
    )
    .bind(run_id)
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let agent_id: Uuid = row.get("agent_id");
    let pid: Option<i32> = row.get("pid");
    let Some(pid) = pid else {
        return Err("agent run does not have a pid yet".to_owned());
    };

    sqlx::query("update agent_runs set status = 'stopping' where id = $1")
        .bind(run_id)
        .execute(pool)
        .await
        .map_err(to_string)?;
    sqlx::query("update agents set status = 'stopping' where id = $1")
        .bind(agent_id)
        .execute(pool)
        .await
        .map_err(to_string)?;

    let status = Command::new("kill")
        .arg("-TERM")
        .arg(format!("-{pid}"))
        .status()
        .await
        .map_err(to_string)?;

    if !status.success() {
        return Err(format!("failed to terminate process group {pid}: {status}"));
    }

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

    tauri::Builder::default()
        .manage(AppState {
            pool,
            db_url: database_url,
        })
        .setup(|app| {
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title("LocalSlock");
            }
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            bootstrap,
            cancel_agent_work,
            create_agent,
            create_channel,
            claim_task,
            delete_agent,
            delete_channel,
            dispatch_agent_work,
            install_supervisor_service,
            mark_channel_read,
            retry_agent_work,
            send_message,
            set_channel_agent_membership,
            start_agent,
            stop_agent,
            uninstall_supervisor_service,
            update_agent,
            update_channel,
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
    use super::{extract_agent_event_json, extract_agent_mentions};

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
}
