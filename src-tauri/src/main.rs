#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::env;

use chrono::{DateTime, Utc};
use serde::Serialize;
use sqlx::{postgres::PgPoolOptions, PgPool, Row};
use tauri::{Manager, State};
use uuid::Uuid;

const DEFAULT_DATABASE_URL: &str = "postgres://dylan:123456@127.0.0.1:5432/localslock";

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
struct Bootstrap {
    db_url: String,
    channels: Vec<Channel>,
    agents: Vec<Agent>,
    messages: Vec<Message>,
    tasks: Vec<Task>,
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

    Ok(())
}

#[tauri::command]
async fn bootstrap(state: State<'_, AppState>) -> CommandResult<Bootstrap> {
    let channels = load_channels(&state.pool).await?;
    let agents = load_agents(&state.pool).await?;
    let messages = load_messages(&state.pool).await?;
    let tasks = load_tasks(&state.pool).await?;

    Ok(Bootstrap {
        db_url: state.db_url.clone(),
        channels,
        agents,
        messages,
        tasks,
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
        insert into agents (handle, display_name, role, status, runtime, model, avatar, description)
        values ($1, $2, 'agent', 'idle', $3, $4, $5, 'Local agent')
        on conflict (handle) do update set
            display_name = excluded.display_name,
            runtime = excluded.runtime,
            model = excluded.model,
            avatar = excluded.avatar
        "#,
    )
    .bind(normalized_handle)
    .bind(display_name)
    .bind(runtime.trim())
    .bind(model.trim())
    .bind(avatar)
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
            description = $7
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
    .execute(&state.pool)
    .await
    .map_err(to_string)?;

    Ok(())
}

#[tauri::command]
async fn delete_agent(agent_id: Uuid, state: State<'_, AppState>) -> CommandResult<()> {
    sqlx::query("delete from agents where id = $1")
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
        select id, handle, display_name, role, status, runtime, model, avatar, description
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
            update_agent,
            update_channel,
            update_task_status
        ])
        .run(tauri::generate_context!())
        .expect("error while running LocalSlock");
}

fn main() {
    run();
}
