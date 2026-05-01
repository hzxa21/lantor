#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::{
    env,
    process::{Command as StdCommand, Stdio},
    time::Duration,
};

use chrono::{DateTime, Utc};
use serde::Serialize;
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
struct Message {
    id: Uuid,
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    sender_name: String,
    sender_role: String,
    body: String,
    is_task: bool,
    task_number: Option<i64>,
    task_status: Option<String>,
    created_at: DateTime<Utc>,
}

#[derive(Debug, Serialize)]
struct Task {
    id: Uuid,
    number: i64,
    title: String,
    status: String,
    channel_name: String,
    assignee_id: Option<Uuid>,
    assignee_name: Option<String>,
}

#[derive(Debug, Serialize)]
struct AgentRun {
    id: Uuid,
    agent_id: Uuid,
    agent_handle: String,
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
struct SupervisorStatus {
    pid: Option<i32>,
    status: String,
    updated_at: Option<DateTime<Utc>>,
}

#[derive(Debug)]
struct SupervisorCommand {
    id: Uuid,
    command_type: String,
    agent_id: Option<Uuid>,
    run_id: Option<Uuid>,
}

#[derive(Debug, Serialize)]
struct Bootstrap {
    db_url: String,
    channels: Vec<Channel>,
    agents: Vec<Agent>,
    messages: Vec<Message>,
    tasks: Vec<Task>,
    agent_runs: Vec<AgentRun>,
    supervisor: SupervisorStatus,
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
            created_at timestamptz not null default now()
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
        create table if not exists supervisor_commands (
            id uuid primary key default gen_random_uuid(),
            command_type text not null,
            agent_id uuid references agents(id) on delete cascade,
            run_id uuid references agent_runs(id) on delete cascade,
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

    Ok(())
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppState>) -> CommandResult<Bootstrap> {
    let channels = load_channels(&state.pool).await?;
    let agents = load_agents(&state.pool).await?;
    let messages = load_messages(&state.pool).await?;
    let tasks = load_tasks(&state.pool).await?;
    let agent_runs = load_agent_runs(&state.pool).await?;
    let supervisor = load_supervisor_status(&state.pool).await?;

    Ok(Bootstrap {
        db_url: state.db_url.clone(),
        channels,
        agents,
        messages,
        tasks,
        agent_runs,
        supervisor,
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
        "#,
    )
    .bind(normalized_handle)
    .bind(display_name)
    .bind(runtime.trim())
    .bind(model.trim())
    .bind(avatar)
    .bind(launch_command.trim())
    .bind(working_directory.trim())
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

    Ok(())
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

    Ok(())
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

    Ok(())
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

    if as_task {
        sqlx::query(
            r#"
            insert into tasks (message_id, channel_id, title, status)
            values ($1, $2, $3, 'todo')
            "#,
        )
        .bind(msg_id)
        .bind(channel_id)
        .bind(body.lines().next().unwrap_or("Untitled task"))
        .execute(&mut *tx)
        .await
        .map_err(to_string)?;
    }

    tx.commit().await.map_err(to_string)?;
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

async fn load_channels(pool: &PgPool) -> CommandResult<Vec<Channel>> {
    let rows = sqlx::query(
        r#"
        select id, name, description, unread_count
        from channels
        order by case when name = 'local-slock' then 0 else 1 end, name
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
            t.title,
            t.status,
            c.name as channel_name,
            t.assignee_agent_id as assignee_id,
            a.display_name as assignee_name
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
            title: row.get("title"),
            status: row.get("status"),
            channel_name: row.get("channel_name"),
            assignee_id: row.get("assignee_id"),
            assignee_name: row.get("assignee_name"),
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

async fn append_run_log(pool: &PgPool, run_id: Uuid, line: String) -> CommandResult<()> {
    sqlx::query("update agent_runs set log = right(log || $2, 20000) where id = $1")
        .bind(run_id)
        .bind(line)
        .execute(pool)
        .await
        .map_err(to_string)?;

    Ok(())
}

async fn pipe_run_output<R>(pool: PgPool, run_id: Uuid, stream: R, label: &'static str)
where
    R: AsyncRead + Unpin,
{
    let mut lines = BufReader::new(stream).lines();
    loop {
        match lines.next_line().await {
            Ok(Some(line)) => {
                let _ = append_run_log(&pool, run_id, format!("[{label}] {line}\n")).await;
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

async fn wait_for_agent_run(
    pool: PgPool,
    agent_id: Uuid,
    run_id: Uuid,
    mut child: tokio::process::Child,
) {
    let result = child.wait().await;
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
    .bind(log_line)
    .execute(&pool)
    .await;

    let _ = sqlx::query("update agents set status = $2 where id = $1")
        .bind(agent_id)
        .bind(agent_status)
        .execute(&pool)
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
        if let Some(command) = claim_next_supervisor_command(&pool).await? {
            let command_id = command.id;
            let result = process_supervisor_command(&pool, command).await;
            finish_supervisor_command(&pool, command_id, result.err()).await?;
        }
        sleep(Duration::from_millis(800)).await;
    }
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
        select id, command_type, agent_id, run_id
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
            supervisor_start_agent(pool, agent_id).await
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

async fn supervisor_start_agent(pool: &PgPool, agent_id: Uuid) -> CommandResult<()> {
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

    let command_text = effective_launch_command(
        row.get("launch_command"),
        row.get("runtime"),
        row.get("model"),
        row.get("handle"),
    );
    let working_directory: String = row.get::<String, _>("working_directory").trim().to_owned();

    let run_id: Uuid = sqlx::query_scalar(
        r#"
        insert into agent_runs (agent_id, command, working_directory, status, log)
        values ($1, $2, $3, 'starting', $4)
        returning id
        "#,
    )
    .bind(agent_id)
    .bind(&command_text)
    .bind(&working_directory)
    .bind(format!("$ {command_text}\n"))
    .fetch_one(pool)
    .await
    .map_err(to_string)?;

    let mut command = Command::new("/bin/zsh");
    command.arg("-lc").arg(&command_text);
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

    if let Some(stdout) = child.stdout.take() {
        tokio::spawn(pipe_run_output(pool.clone(), run_id, stdout, "stdout"));
    }
    if let Some(stderr) = child.stderr.take() {
        tokio::spawn(pipe_run_output(pool.clone(), run_id, stderr, "stderr"));
    }

    tokio::spawn(wait_for_agent_run(pool.clone(), agent_id, run_id, child));

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
            create_agent,
            create_channel,
            claim_task,
            delete_agent,
            delete_channel,
            send_message,
            start_agent,
            stop_agent,
            update_agent,
            update_channel,
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
