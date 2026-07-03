use std::{
    collections::HashSet,
    convert::Infallible,
    env,
    net::SocketAddr,
    path::{Path, PathBuf},
    sync::Arc,
};

use axum::{
    body::Body,
    extract::{DefaultBodyLimit, Path as AxumPath, State},
    http::{header, HeaderMap, HeaderValue, Request, StatusCode},
    middleware::{self, Next},
    response::{
        sse::{Event, KeepAlive},
        IntoResponse, Response, Sse,
    },
    routing::{get, post},
    Json, Router,
};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::json;
use sha2::{Digest, Sha256};
use sqlx::{Row, SqlitePool};
use tokio::{
    net::TcpListener,
    sync::Mutex,
    time::{sleep, Duration},
};
use tower_http::{
    compression::CompressionLayer,
    services::{ServeDir, ServeFile},
};
use uuid::Uuid;

use crate::agent_profile::{
    create_agent_in_pool, delete_agent_in_pool, update_agent_in_pool, update_owner_profile_in_pool,
};
use crate::agent_workspace::{agent_workspace_list_in_pool, agent_workspace_read_file_in_pool};
use crate::bootstrap::load_web_bootstrap;
use crate::channels::{
    add_agent_to_channel, create_channel_in_pool, delete_channel_in_pool,
    open_dm_with_agent_in_pool, set_channel_agent_membership_in_pool, update_channel_in_pool,
};
use crate::domain::reminders::complete_reminder_in_pool;
use crate::launch_agent;
use crate::lifecycle_commands::start_agent_in_pool;
use crate::message_store::{load_artifact, send_owner_message_in_pool, set_message_saved_in_pool};
use crate::models::AttachmentUpload;
use crate::owner_inbox::{
    dismiss_inbox_items_in_pool, mark_all_owner_inbox_read_in_pool, mark_channel_read_in_pool,
    mark_inbox_items_read_in_pool,
};
use crate::system_commands::check_runtime_in_env;
use crate::task_store::{update_task_status_in_pool, update_task_title_in_pool};
use crate::ui_notifications::notify_ui_refresh;
use crate::{
    app::to_string, cancel_agent_work_in_pool, claim_task_in_pool, retry_agent_work_in_pool,
};

const WEB_SEND_MESSAGE_BODY_LIMIT: usize = 128 * 1024 * 1024;
const WEB_AUTH_COOKIE: &str = "lantor_web_session";
const WEB_AUTH_STATE_ID: &str = "web_pin";
const DEFAULT_WEB_PIN_MAX_FAILURES: i64 = 10;

#[derive(Clone)]
struct WebState {
    pool: SqlitePool,
    db_url: String,
    auth: Arc<WebAuth>,
}

struct WebAuth {
    max_failures: i64,
    sessions: Mutex<HashSet<String>>,
}

#[derive(Serialize)]
struct ApiError {
    ok: bool,
    message: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AuthLoginRequest {
    pin: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct WebAuthStatusPayload {
    ok: bool,
    required: bool,
    authenticated: bool,
    locked: bool,
    failed_attempts: i64,
    max_failures: i64,
    unlock_command: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetWebPinRequest {
    pin: String,
    current_pin: Option<String>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendMessageRequest {
    channel_id: Uuid,
    thread_root_id: Option<Uuid>,
    body: String,
    as_task: bool,
    attachments: Option<Vec<AttachmentUpload>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct RuntimeCheckRequest {
    runtime: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChannelIdRequest {
    channel_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateChannelRequest {
    name: String,
    #[serde(default)]
    description: Option<String>,
    #[serde(default)]
    agent_ids: Option<Vec<Uuid>>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateChannelRequest {
    channel_id: Uuid,
    name: String,
    description: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetChannelAgentMembershipRequest {
    channel_id: Uuid,
    agent_id: Uuid,
    member: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ReminderIdRequest {
    reminder_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DismissInboxItemRequest {
    item_id: String,
    dismissed_until: DateTime<Utc>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct DismissInboxItemsRequest {
    items: Vec<DismissInboxItemRequest>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ArtifactReadRequest {
    artifact_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SetMessageSavedRequest {
    message_id: Uuid,
    saved: bool,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentIdRequest {
    agent_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct WorkItemIdRequest {
    work_item_id: Uuid,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskStatusRequest {
    task_id: Uuid,
    status: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct TaskTitleRequest {
    task_id: Uuid,
    title: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaimTaskRequest {
    task_id: Uuid,
    agent_id: Option<Uuid>,
    expected_version: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateAgentRequest {
    handle: String,
    display_name: String,
    role: Option<String>,
    runtime: String,
    model: String,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    avatar: Option<String>,
    description: Option<String>,
    launch_command: String,
    environment_variables: Option<String>,
    working_directory: String,
    daily_budget_micros: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct UpdateAgentRequest {
    agent_id: Uuid,
    handle: String,
    display_name: String,
    role: Option<String>,
    runtime: String,
    model: String,
    reasoning_effort: Option<String>,
    service_tier: Option<String>,
    avatar: Option<String>,
    description: String,
    launch_command: String,
    environment_variables: Option<String>,
    working_directory: String,
    daily_budget_micros: Option<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct AgentWorkspaceRequest {
    agent_id: Uuid,
    path: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct OwnerProfileRequest {
    display_name: String,
    avatar: String,
    description: String,
}

pub(crate) const DEFAULT_LANTOR_WEB_BIND: &str = "0.0.0.0:8787";

pub(crate) fn resolve_web_bind() -> Option<String> {
    match env::var("LANTOR_WEB_BIND") {
        Ok(value) => {
            let trimmed = value.trim().to_owned();
            if trimmed.is_empty() {
                return Some(DEFAULT_LANTOR_WEB_BIND.to_owned());
            }
            if matches!(
                trimmed.to_ascii_lowercase().as_str(),
                "off" | "none" | "disabled" | "false" | "0"
            ) {
                return None;
            }
            Some(trimmed)
        }
        Err(_) => Some(DEFAULT_LANTOR_WEB_BIND.to_owned()),
    }
}

pub(crate) fn spawn_web_server_if_configured(pool: SqlitePool, db_url: String) {
    let Some(bind) = resolve_web_bind() else {
        return;
    };
    let Ok(addr) = bind.parse::<SocketAddr>() else {
        eprintln!("Lantor web access disabled: invalid LANTOR_WEB_BIND={bind}");
        return;
    };

    let dist_dir = web_dist_dir();
    tauri::async_runtime::spawn(async move {
        let auth = match resolve_web_auth(&pool).await {
            Ok(auth) => auth,
            Err(err) => {
                eprintln!("Lantor web access disabled: {err}");
                return;
            }
        };
        let state = Arc::new(WebState { pool, db_url, auth });
        let app = web_router(state, dist_dir);
        match TcpListener::bind(addr).await {
            Ok(listener) => {
                eprintln!("Lantor web access listening on http://{addr}");
                if let Err(err) = axum::serve(
                    listener,
                    app.into_make_service_with_connect_info::<SocketAddr>(),
                )
                .await
                {
                    eprintln!("Lantor web access stopped: {err}");
                }
            }
            Err(err) => {
                eprintln!("Lantor web access failed to bind {addr}: {err}");
            }
        }
    });
}

async fn resolve_web_auth(pool: &SqlitePool) -> Result<Arc<WebAuth>, String> {
    seed_web_auth_pin_from_env(pool, true).await?;
    let max_failures = configured_web_pin_max_failures();
    Ok(Arc::new(WebAuth {
        max_failures,
        sessions: Mutex::new(HashSet::new()),
    }))
}

async fn seed_web_auth_pin_from_env(pool: &SqlitePool, reject_invalid: bool) -> Result<(), String> {
    ensure_web_auth_state(pool).await?;
    if web_auth_pin_hash(pool).await?.is_none() {
        if let Ok(value) = env::var("LANTOR_WEB_PIN") {
            let pin = value.trim();
            if !valid_pin(pin) {
                if reject_invalid {
                    return Err("LANTOR_WEB_PIN must be exactly 6 digits".to_owned());
                }
                return Ok(());
            }
            write_web_auth_pin_hash(pool, Some(&hash_pin(pin))).await?;
        }
    }
    Ok(())
}

async fn ensure_web_auth_state(pool: &SqlitePool) -> Result<(), String> {
    sqlx::query(
        r#"
        create table if not exists web_auth_state (
            id text primary key not null,
            pin_hash text,
            failed_attempts integer not null default 0,
            locked_at text
        )
        "#,
    )
    .execute(pool)
    .await
    .map_err(to_string)?;

    let has_pin_hash = sqlx::query("pragma table_info(web_auth_state)")
        .fetch_all(pool)
        .await
        .map_err(to_string)?
        .into_iter()
        .any(|row| row.get::<String, _>("name") == "pin_hash");
    if !has_pin_hash {
        sqlx::query("alter table web_auth_state add column pin_hash text")
            .execute(pool)
            .await
            .map_err(to_string)?;
    }

    sqlx::query(
        r#"
        insert into web_auth_state (id, pin_hash, failed_attempts, locked_at)
        values (?1, null, 0, null)
        on conflict(id) do nothing
        "#,
    )
    .bind(WEB_AUTH_STATE_ID)
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

fn valid_pin(pin: &str) -> bool {
    pin.len() == 6 && pin.chars().all(|ch| ch.is_ascii_digit())
}

fn configured_web_pin_max_failures() -> i64 {
    env::var("LANTOR_WEB_PIN_MAX_FAILURES")
        .ok()
        .and_then(|value| value.trim().parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_WEB_PIN_MAX_FAILURES)
}

fn hash_pin(pin: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(pin.as_bytes());
    format!("{:x}", hasher.finalize())
}

fn web_router(state: Arc<WebState>, dist_dir: PathBuf) -> Router {
    let index = dist_dir.join("index.html");
    let protected_api = Router::new()
        .route(
            "/bootstrap",
            get(api_bootstrap).layer(CompressionLayer::new()),
        )
        .route("/check_runtime", post(api_check_runtime))
        .route("/events", get(api_events))
        .route("/attachments/{attachment_id}", get(api_attachment))
        .route(
            "/send_message",
            post(api_send_message).layer(DefaultBodyLimit::max(WEB_SEND_MESSAGE_BODY_LIMIT)),
        )
        .route("/create_channel", post(api_create_channel))
        .route("/update_channel", post(api_update_channel))
        .route("/delete_channel", post(api_delete_channel))
        .route("/create_agent", post(api_create_agent))
        .route("/update_agent", post(api_update_agent))
        .route("/delete_agent", post(api_delete_agent))
        .route("/start_agent", post(api_start_agent))
        .route(
            "/set_channel_agent_membership",
            post(api_set_channel_agent_membership),
        )
        .route("/set_message_saved", post(api_set_message_saved))
        .route("/update_owner_profile", post(api_update_owner_profile))
        .route("/dismiss_inbox_items", post(api_dismiss_inbox_items))
        .route("/mark_inbox_items_read", post(api_mark_inbox_items_read))
        .route("/mark_all_inbox_read", post(api_mark_all_inbox_read))
        .route("/mark_channel_read", post(api_mark_channel_read))
        .route("/complete_reminder", post(api_complete_reminder))
        .route("/update_task_status", post(api_update_task_status))
        .route("/update_task_title", post(api_update_task_title))
        .route("/claim_task", post(api_claim_task))
        .route("/cancel_agent_work", post(api_cancel_agent_work))
        .route("/retry_agent_work", post(api_retry_agent_work))
        .route(
            "/install_supervisor_service",
            post(api_install_supervisor_service),
        )
        .route(
            "/uninstall_supervisor_service",
            post(api_uninstall_supervisor_service),
        )
        .route("/artifact_read", post(api_artifact_read))
        .route("/open_dm_with_agent", post(api_open_dm_with_agent))
        .route("/agent_workspace_list", post(api_agent_workspace_list))
        .route(
            "/agent_workspace_read_file",
            post(api_agent_workspace_read_file),
        )
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            require_web_auth,
        ))
        .with_state(state.clone());

    let app = Router::new()
        .route("/api/health", get(api_health))
        .route("/api/auth/status", get(api_auth_status))
        .route("/api/auth/login", post(api_auth_login))
        .route("/api/auth/logout", post(api_auth_logout))
        .route("/api/auth/set_pin", post(api_auth_set_pin))
        .nest("/api", protected_api)
        .with_state(state);

    if index.is_file() {
        app.fallback_service(ServeDir::new(&dist_dir).fallback(ServeFile::new(index)))
    } else {
        app.fallback(get(move || missing_dist(dist_dir)))
    }
}

fn web_dist_dir() -> PathBuf {
    if let Ok(path) = env::var("LANTOR_WEB_DIST") {
        let path = PathBuf::from(path);
        if path.is_dir() {
            return path;
        }
    }
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let candidates = [
        manifest_dir.join("../dist"),
        env::current_dir()
            .unwrap_or_else(|_| PathBuf::from("."))
            .join("dist"),
    ];
    candidates
        .into_iter()
        .find(|path| path.join("index.html").is_file())
        .unwrap_or_else(|| manifest_dir.join("../dist"))
}

async fn missing_dist(dist_dir: PathBuf) -> impl IntoResponse {
    let body = format!(
        r#"<!doctype html>
<html>
  <head><title>Lantor Web</title></head>
  <body style="font-family: -apple-system, BlinkMacSystemFont, sans-serif; padding: 32px;">
    <h1>Lantor Web build not found</h1>
    <p>Expected <code>{}</code>.</p>
    <p>Run <code>npm run build</code>, then restart Lantor.</p>
  </body>
</html>"#,
        dist_dir.display()
    );
    (
        StatusCode::SERVICE_UNAVAILABLE,
        [(header::CONTENT_TYPE, "text/html; charset=utf-8")],
        body,
    )
}

async fn api_health() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn api_auth_status(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<impl IntoResponse, Response> {
    web_auth_status_payload(
        &state.pool,
        &state.db_url,
        state.auth.max_failures,
        web_session_authenticated(&state, &headers).await,
    )
    .await
    .map(Json)
    .map_err(api_error)
}

async fn api_auth_login(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(request): Json<AuthLoginRequest>,
) -> Result<Response, Response> {
    let Some(pin_hash) = web_auth_pin_hash(&state.pool).await.map_err(api_error)? else {
        return Ok(Json(json!({ "ok": true, "required": false })).into_response());
    };
    if web_session_authenticated(&state, &headers).await {
        return Ok(Json(json!({ "ok": true, "authenticated": true })).into_response());
    }

    let (_, locked) = web_auth_lock_status(&state.pool).await.map_err(api_error)?;
    if locked {
        return Err(web_auth_locked_response(&state.db_url));
    }
    if !pin_matches(request.pin.trim(), &pin_hash) {
        let (failed_attempts, locked) =
            web_auth_record_failure(&state.pool, state.auth.max_failures)
                .await
                .map_err(api_error)?;
        if locked {
            return Err(web_auth_locked_response(&state.db_url));
        }
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "ok": false,
                "message": "Invalid PIN",
                "failedAttempts": failed_attempts,
                "maxFailures": state.auth.max_failures,
            })),
        )
            .into_response());
    }

    web_auth_clear_failures(&state.pool)
        .await
        .map_err(api_error)?;
    let session = Uuid::new_v4().simple().to_string();
    state.auth.sessions.lock().await.insert(session.clone());
    let mut response = Json(json!({ "ok": true, "authenticated": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{WEB_AUTH_COOKIE}={session}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800"
        ))
        .map_err(to_string)
        .map_err(api_error)?,
    );
    Ok(response)
}

async fn api_auth_set_pin(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
    Json(request): Json<SetWebPinRequest>,
) -> Result<Response, Response> {
    let pin_is_configured = web_auth_pin_hash(&state.pool)
        .await
        .map_err(api_error)?
        .is_some();
    if pin_is_configured && !web_session_authenticated(&state, &headers).await {
        return Err((
            StatusCode::UNAUTHORIZED,
            Json(json!({
                "ok": false,
                "message": "PIN login required",
                "authRequired": true,
            })),
        )
            .into_response());
    }
    let status = set_web_pin_in_pool(
        &state.pool,
        &state.db_url,
        state.auth.max_failures,
        request.pin,
        request.current_pin,
        true,
    )
    .await
    .map_err(api_error)?;
    let session = Uuid::new_v4().simple().to_string();
    state.auth.sessions.lock().await.insert(session.clone());
    let mut response = Json(status).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_str(&format!(
            "{WEB_AUTH_COOKIE}={session}; Path=/; HttpOnly; SameSite=Lax; Max-Age=604800"
        ))
        .map_err(to_string)
        .map_err(api_error)?,
    );
    Ok(response)
}

async fn api_auth_logout(
    State(state): State<Arc<WebState>>,
    headers: HeaderMap,
) -> Result<Response, Response> {
    if let Some(session) = cookie_value(&headers, WEB_AUTH_COOKIE) {
        state.auth.sessions.lock().await.remove(&session);
    }
    let mut response = Json(json!({ "ok": true })).into_response();
    response.headers_mut().insert(
        header::SET_COOKIE,
        HeaderValue::from_static("lantor_web_session=; Path=/; HttpOnly; SameSite=Lax; Max-Age=0"),
    );
    Ok(response)
}

async fn api_bootstrap(State(state): State<Arc<WebState>>) -> Result<impl IntoResponse, Response> {
    load_web_bootstrap(&state.pool, state.db_url.clone())
        .await
        .map(Json)
        .map_err(api_error)
}

async fn api_check_runtime(
    Json(request): Json<RuntimeCheckRequest>,
) -> Result<impl IntoResponse, Response> {
    check_runtime_in_env(request.runtime)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn api_send_message(
    State(state): State<Arc<WebState>>,
    Json(request): Json<SendMessageRequest>,
) -> Result<impl IntoResponse, Response> {
    send_owner_message_in_pool(
        &state.pool,
        request.channel_id,
        request.thread_root_id,
        &request.body,
        request.as_task,
        request.attachments.unwrap_or_default(),
    )
    .await
    .map(Json)
    .map_err(api_error)
}

async fn api_create_channel(
    State(state): State<Arc<WebState>>,
    Json(request): Json<CreateChannelRequest>,
) -> Result<impl IntoResponse, Response> {
    let description = request.description.unwrap_or_default();
    let channel_id = create_channel_in_pool(&state.pool, &request.name, &description)
        .await
        .map_err(api_error)?;
    if let Some(ids) = request.agent_ids {
        let mut seen = std::collections::HashSet::new();
        for agent_id in ids {
            if !seen.insert(agent_id) {
                continue;
            }
            add_agent_to_channel(&state.pool, channel_id, agent_id)
                .await
                .map_err(api_error)?;
        }
    }
    Ok(Json(json!({ "ok": true, "channelId": channel_id })))
}

async fn api_update_channel(
    State(state): State<Arc<WebState>>,
    Json(request): Json<UpdateChannelRequest>,
) -> Result<impl IntoResponse, Response> {
    update_channel_in_pool(
        &state.pool,
        request.channel_id,
        request.name,
        request.description,
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_delete_channel(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ChannelIdRequest>,
) -> Result<impl IntoResponse, Response> {
    delete_channel_in_pool(&state.pool, request.channel_id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_create_agent(
    State(state): State<Arc<WebState>>,
    Json(request): Json<CreateAgentRequest>,
) -> Result<impl IntoResponse, Response> {
    create_agent_in_pool(
        &state.pool,
        request.handle,
        request.display_name,
        request.role,
        request.runtime,
        request.model,
        request.reasoning_effort,
        request.service_tier,
        request.avatar,
        request.description,
        request.launch_command,
        request.environment_variables,
        request.working_directory,
        request.daily_budget_micros,
    )
    .await
    .map(|agent_id| Json(agent_id.to_string()))
    .map_err(api_error)
}

async fn api_update_agent(
    State(state): State<Arc<WebState>>,
    Json(request): Json<UpdateAgentRequest>,
) -> Result<impl IntoResponse, Response> {
    update_agent_in_pool(
        &state.pool,
        request.agent_id,
        request.handle,
        request.display_name,
        request.role,
        request.runtime,
        request.model,
        request.reasoning_effort,
        request.service_tier,
        request.avatar,
        request.description,
        request.launch_command,
        request.environment_variables,
        request.working_directory,
        request.daily_budget_micros,
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_delete_agent(
    State(state): State<Arc<WebState>>,
    Json(request): Json<AgentIdRequest>,
) -> Result<impl IntoResponse, Response> {
    delete_agent_in_pool(&state.pool, request.agent_id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_start_agent(
    State(state): State<Arc<WebState>>,
    Json(request): Json<AgentIdRequest>,
) -> Result<impl IntoResponse, Response> {
    start_agent_in_pool(&state.pool, request.agent_id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_set_channel_agent_membership(
    State(state): State<Arc<WebState>>,
    Json(request): Json<SetChannelAgentMembershipRequest>,
) -> Result<impl IntoResponse, Response> {
    set_channel_agent_membership_in_pool(
        &state.pool,
        request.channel_id,
        request.agent_id,
        request.member,
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_update_owner_profile(
    State(state): State<Arc<WebState>>,
    Json(request): Json<OwnerProfileRequest>,
) -> Result<impl IntoResponse, Response> {
    update_owner_profile_in_pool(
        &state.pool,
        request.display_name,
        request.avatar,
        request.description,
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_mark_channel_read(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ChannelIdRequest>,
) -> Result<impl IntoResponse, Response> {
    mark_channel_read_in_pool(&state.pool, request.channel_id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_dismiss_inbox_items(
    State(state): State<Arc<WebState>>,
    Json(request): Json<DismissInboxItemsRequest>,
) -> Result<impl IntoResponse, Response> {
    dismiss_inbox_items_in_pool(
        &state.pool,
        request
            .items
            .into_iter()
            .map(|item| (item.item_id, item.dismissed_until)),
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_mark_inbox_items_read(
    State(state): State<Arc<WebState>>,
    Json(request): Json<DismissInboxItemsRequest>,
) -> Result<impl IntoResponse, Response> {
    mark_inbox_items_read_in_pool(
        &state.pool,
        request
            .items
            .into_iter()
            .map(|item| (item.item_id, item.dismissed_until)),
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_mark_all_inbox_read(
    State(state): State<Arc<WebState>>,
) -> Result<impl IntoResponse, Response> {
    mark_all_owner_inbox_read_in_pool(&state.pool)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_set_message_saved(
    State(state): State<Arc<WebState>>,
    Json(request): Json<SetMessageSavedRequest>,
) -> Result<impl IntoResponse, Response> {
    set_message_saved_in_pool(&state.pool, request.message_id, request.saved)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_complete_reminder(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ReminderIdRequest>,
) -> Result<impl IntoResponse, Response> {
    complete_reminder_in_pool(&state.pool, request.reminder_id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_update_task_status(
    State(state): State<Arc<WebState>>,
    Json(request): Json<TaskStatusRequest>,
) -> Result<impl IntoResponse, Response> {
    update_task_status_in_pool(&state.pool, request.task_id, request.status)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_update_task_title(
    State(state): State<Arc<WebState>>,
    Json(request): Json<TaskTitleRequest>,
) -> Result<impl IntoResponse, Response> {
    update_task_title_in_pool(&state.pool, request.task_id, request.title)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_claim_task(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ClaimTaskRequest>,
) -> Result<impl IntoResponse, Response> {
    claim_task_in_pool(
        &state.pool,
        request.task_id,
        request.agent_id,
        request.expected_version,
    )
    .await
    .map(|_| Json(json!({ "ok": true })))
    .map_err(api_error)
}

async fn api_cancel_agent_work(
    State(state): State<Arc<WebState>>,
    Json(request): Json<WorkItemIdRequest>,
) -> Result<impl IntoResponse, Response> {
    cancel_agent_work_in_pool(&state.pool, request.work_item_id)
        .await
        .map(|_| Json(json!({ "ok": true })))
        .map_err(api_error)
}

async fn api_retry_agent_work(
    State(state): State<Arc<WebState>>,
    Json(request): Json<WorkItemIdRequest>,
) -> Result<impl IntoResponse, Response> {
    retry_agent_work_in_pool(&state.pool, request.work_item_id)
        .await
        .map(|work_item_id| Json(json!({ "workItemId": work_item_id })))
        .map_err(api_error)
}

async fn api_install_supervisor_service(
    State(state): State<Arc<WebState>>,
) -> Result<impl IntoResponse, Response> {
    let status = launch_agent::install_supervisor_service(&state.db_url).map_err(api_error)?;
    let _ = notify_ui_refresh(&state.pool, "supervisor_service_installed").await;
    Ok(Json(status))
}

async fn api_uninstall_supervisor_service(
    State(state): State<Arc<WebState>>,
) -> Result<impl IntoResponse, Response> {
    let status = launch_agent::uninstall_supervisor_service().map_err(api_error)?;
    sqlx::query("update supervisor_state set status = 'offline', updated_at = strftime('%Y-%m-%dT%H:%M:%f+00:00','now') where id = 1")
        .execute(&state.pool)
        .await
        .map_err(to_string)
        .map_err(api_error)?;
    let _ = notify_ui_refresh(&state.pool, "supervisor_service_uninstalled").await;
    Ok(Json(status))
}

async fn api_artifact_read(
    State(state): State<Arc<WebState>>,
    Json(request): Json<ArtifactReadRequest>,
) -> Result<impl IntoResponse, Response> {
    load_artifact(&state.pool, request.artifact_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn api_open_dm_with_agent(
    State(state): State<Arc<WebState>>,
    Json(request): Json<AgentIdRequest>,
) -> Result<impl IntoResponse, Response> {
    open_dm_with_agent_in_pool(&state.pool, request.agent_id)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn api_agent_workspace_list(
    State(state): State<Arc<WebState>>,
    Json(request): Json<AgentWorkspaceRequest>,
) -> Result<impl IntoResponse, Response> {
    agent_workspace_list_in_pool(&state.pool, request.agent_id, &request.path)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn api_agent_workspace_read_file(
    State(state): State<Arc<WebState>>,
    Json(request): Json<AgentWorkspaceRequest>,
) -> Result<impl IntoResponse, Response> {
    agent_workspace_read_file_in_pool(&state.pool, request.agent_id, &request.path)
        .await
        .map(Json)
        .map_err(api_error)
}

async fn api_events(State(state): State<Arc<WebState>>) -> Result<impl IntoResponse, Response> {
    let pool = state.pool.clone();
    let stream = async_stream::stream! {
        let mut last_id: i64 = sqlx::query_scalar("select coalesce(max(id), 0) from ui_events")
            .fetch_one(&pool)
            .await
            .unwrap_or(0);
        loop {
            match sqlx::query(
                r#"
                select id, event_json
                from ui_events
                where id > $1
                order by id asc
                limit 80
                "#,
            )
            .bind(last_id)
            .fetch_all(&pool)
            .await {
                Ok(rows) if rows.is_empty() => {
                    sleep(Duration::from_millis(500)).await;
                }
                Ok(rows) => {
                    for row in rows {
                        last_id = row.get("id");
                        yield Ok::<Event, Infallible>(
                            Event::default().event("lantor").data(row.get::<String, _>("event_json"))
                        );
                    }
                },
                Err(err) => {
                    yield Ok(Event::default().event("error").data(err.to_string()));
                    sleep(Duration::from_secs(2)).await;
                },
            }
        }
    };
    Ok(Sse::new(stream).keep_alive(KeepAlive::default()))
}

async fn api_attachment(
    State(state): State<Arc<WebState>>,
    AxumPath(attachment_id): AxumPath<Uuid>,
) -> Result<Response, Response> {
    let row = sqlx::query(
        r#"
        select original_name, mime_type, storage_path
        from message_attachments
        where id = $1
        "#,
    )
    .bind(attachment_id)
    .fetch_optional(&state.pool)
    .await
    .map_err(to_string)
    .map_err(api_error)?
    .ok_or_else(|| api_error("attachment does not exist".to_owned()))?;

    let original_name: String = row.get("original_name");
    let mime_type: String = row.get("mime_type");
    let storage_path: String = row.get("storage_path");
    let bytes = tokio::fs::read(Path::new(&storage_path))
        .await
        .map_err(to_string)
        .map_err(api_error)?;
    let content_type = if mime_type.trim().is_empty() {
        mime_guess::from_path(&storage_path)
            .first_or_octet_stream()
            .to_string()
    } else {
        mime_type
    };
    let mut response = Response::new(Body::from(bytes));
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(&content_type)
            .unwrap_or(HeaderValue::from_static("application/octet-stream")),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "inline; filename=\"{}\"",
            original_name.replace('"', "")
        ))
        .unwrap_or(HeaderValue::from_static("inline")),
    );
    Ok(response)
}

async fn require_web_auth(
    State(state): State<Arc<WebState>>,
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    let pin_is_configured = web_auth_pin_hash(&state.pool)
        .await
        .map_err(api_error)?
        .is_some();
    if !pin_is_configured || web_session_authenticated(&state, request.headers()).await {
        return Ok(next.run(request).await);
    }
    Err((
        StatusCode::UNAUTHORIZED,
        Json(json!({
            "ok": false,
            "message": "PIN login required",
            "authRequired": true,
        })),
    )
        .into_response())
}

async fn web_session_authenticated(state: &WebState, headers: &HeaderMap) -> bool {
    let Some(session) = cookie_value(headers, WEB_AUTH_COOKIE) else {
        return false;
    };
    state.auth.sessions.lock().await.contains(&session)
}

fn cookie_value(headers: &HeaderMap, name: &str) -> Option<String> {
    let cookie = headers.get(header::COOKIE)?.to_str().ok()?;
    cookie.split(';').find_map(|part| {
        let (key, value) = part.trim().split_once('=')?;
        (key == name).then(|| value.to_owned())
    })
}

async fn web_auth_lock_status(pool: &SqlitePool) -> Result<(i64, bool), String> {
    ensure_web_auth_state(pool).await?;
    let row = sqlx::query("select failed_attempts, locked_at from web_auth_state where id = ?1")
        .bind(WEB_AUTH_STATE_ID)
        .fetch_one(pool)
        .await
        .map_err(to_string)?;
    let failed_attempts: i64 = row.get("failed_attempts");
    let locked_at: Option<String> = row.get("locked_at");
    Ok((failed_attempts, locked_at.is_some()))
}

async fn web_auth_status_payload(
    pool: &SqlitePool,
    db_url: &str,
    max_failures: i64,
    authenticated: bool,
) -> Result<WebAuthStatusPayload, String> {
    let required = web_auth_pin_hash(pool).await?.is_some();
    let (failed_attempts, locked) = web_auth_lock_status(pool).await?;
    Ok(WebAuthStatusPayload {
        ok: true,
        required,
        authenticated: !required || authenticated,
        locked,
        failed_attempts,
        max_failures,
        unlock_command: locked.then(|| web_auth_unlock_command(db_url)),
    })
}

async fn web_auth_pin_hash(pool: &SqlitePool) -> Result<Option<String>, String> {
    ensure_web_auth_state(pool).await?;
    sqlx::query_scalar("select pin_hash from web_auth_state where id = ?1")
        .bind(WEB_AUTH_STATE_ID)
        .fetch_optional(pool)
        .await
        .map_err(to_string)
        .map(|value| value.flatten())
}

async fn write_web_auth_pin_hash(pool: &SqlitePool, pin_hash: Option<&str>) -> Result<(), String> {
    ensure_web_auth_state(pool).await?;
    sqlx::query(
        "update web_auth_state set pin_hash = ?2, failed_attempts = 0, locked_at = null where id = ?1",
    )
    .bind(WEB_AUTH_STATE_ID)
    .bind(pin_hash)
    .execute(pool)
    .await
    .map_err(to_string)?;
    Ok(())
}

fn pin_matches(pin: &str, pin_hash: &str) -> bool {
    valid_pin(pin) && hash_pin(pin) == pin_hash
}

#[tauri::command]
pub(crate) async fn web_auth_status(
    state: tauri::State<'_, crate::app::AppState>,
) -> crate::app::CommandResult<WebAuthStatusPayload> {
    seed_web_auth_pin_from_env(&state.pool, false).await?;
    web_auth_status_payload(
        &state.pool,
        state.db_url(),
        configured_web_pin_max_failures(),
        true,
    )
    .await
}

#[tauri::command]
pub(crate) async fn set_web_pin(
    state: tauri::State<'_, crate::app::AppState>,
    pin: String,
    current_pin: Option<String>,
) -> crate::app::CommandResult<WebAuthStatusPayload> {
    seed_web_auth_pin_from_env(&state.pool, false).await?;
    set_web_pin_in_pool(
        &state.pool,
        state.db_url(),
        configured_web_pin_max_failures(),
        pin,
        current_pin,
        true,
    )
    .await
}

pub(crate) async fn set_web_pin_in_pool(
    pool: &SqlitePool,
    db_url: &str,
    max_failures: i64,
    pin: String,
    current_pin: Option<String>,
    require_current_when_configured: bool,
) -> Result<WebAuthStatusPayload, String> {
    let pin = pin.trim();
    if !valid_pin(pin) {
        return Err("PIN must be exactly 6 digits".to_owned());
    }
    let existing_pin_hash = web_auth_pin_hash(pool).await?;
    if require_current_when_configured {
        if let Some(existing_pin_hash) = existing_pin_hash.as_deref() {
            let current_pin = current_pin
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .ok_or_else(|| "Current PIN is required".to_owned())?;
            if !pin_matches(current_pin, existing_pin_hash) {
                return Err("Current PIN is incorrect".to_owned());
            }
        }
    }
    write_web_auth_pin_hash(pool, Some(&hash_pin(pin))).await?;
    web_auth_status_payload(pool, db_url, max_failures, true).await
}

async fn web_auth_record_failure(
    pool: &SqlitePool,
    max_failures: i64,
) -> Result<(i64, bool), String> {
    ensure_web_auth_state(pool).await?;
    sqlx::query(
        r#"
        update web_auth_state
        set
            failed_attempts = failed_attempts + 1,
            locked_at = case
                when failed_attempts + 1 >= ?2 then strftime('%Y-%m-%dT%H:%M:%f+00:00','now')
                else locked_at
            end
        where id = ?1 and locked_at is null
        "#,
    )
    .bind(WEB_AUTH_STATE_ID)
    .bind(max_failures)
    .execute(pool)
    .await
    .map_err(to_string)?;
    web_auth_lock_status(pool).await
}

async fn web_auth_clear_failures(pool: &SqlitePool) -> Result<(), String> {
    ensure_web_auth_state(pool).await?;
    sqlx::query("update web_auth_state set failed_attempts = 0, locked_at = null where id = ?1")
        .bind(WEB_AUTH_STATE_ID)
        .execute(pool)
        .await
        .map_err(to_string)?;
    Ok(())
}

fn web_auth_locked_response(db_url: &str) -> Response {
    (
        StatusCode::LOCKED,
        Json(json!({
            "ok": false,
            "message": "PIN login locked after too many failed attempts. Run the unlock command on the Lantor host to allow more attempts.",
            "locked": true,
            "unlockCommand": web_auth_unlock_command(db_url),
        })),
    )
        .into_response()
}

fn web_auth_unlock_command(db_url: &str) -> String {
    let database = sqlite_path_from_url(db_url).unwrap_or_else(|| db_url.to_owned());
    format!(
        "sqlite3 {} \"update web_auth_state set failed_attempts=0, locked_at=null where id='web_pin';\"",
        shell_quote(&database)
    )
}

fn sqlite_path_from_url(db_url: &str) -> Option<String> {
    db_url
        .strip_prefix("sqlite://")
        .or_else(|| db_url.strip_prefix("sqlite:"))
        .map(str::to_owned)
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn api_error(message: String) -> Response {
    (
        StatusCode::BAD_REQUEST,
        Json(ApiError { ok: false, message }),
    )
        .into_response()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn web_auth_failure_lock_persists_until_cleared() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect memory sqlite");

        assert_eq!(
            web_auth_lock_status(&pool).await.expect("status"),
            (0, false)
        );
        assert_eq!(
            web_auth_record_failure(&pool, 2)
                .await
                .expect("first failure"),
            (1, false)
        );
        assert_eq!(
            web_auth_record_failure(&pool, 2)
                .await
                .expect("second failure"),
            (2, true)
        );
        assert_eq!(
            web_auth_record_failure(&pool, 2)
                .await
                .expect("locked failure"),
            (2, true)
        );

        web_auth_clear_failures(&pool)
            .await
            .expect("clear failures");
        assert_eq!(
            web_auth_lock_status(&pool).await.expect("cleared"),
            (0, false)
        );
    }

    #[tokio::test]
    async fn set_web_pin_requires_current_pin_when_configured() {
        let pool = SqlitePool::connect("sqlite::memory:")
            .await
            .expect("connect memory sqlite");

        set_web_pin_in_pool(
            &pool,
            "sqlite::memory:",
            10,
            "123456".to_owned(),
            None,
            true,
        )
        .await
        .expect("initial pin");
        let current_hash = web_auth_pin_hash(&pool)
            .await
            .expect("pin hash")
            .expect("configured pin hash");
        assert!(pin_matches("123456", &current_hash));

        let err = set_web_pin_in_pool(
            &pool,
            "sqlite::memory:",
            10,
            "654321".to_owned(),
            Some("000000".to_owned()),
            true,
        )
        .await
        .expect_err("wrong current pin should fail");
        assert_eq!(err, "Current PIN is incorrect");

        set_web_pin_in_pool(
            &pool,
            "sqlite::memory:",
            10,
            "654321".to_owned(),
            Some("123456".to_owned()),
            true,
        )
        .await
        .expect("changed pin");
        let next_hash = web_auth_pin_hash(&pool)
            .await
            .expect("next pin hash")
            .expect("configured next pin hash");
        assert!(pin_matches("654321", &next_hash));
    }

    #[test]
    fn cookie_value_reads_named_cookie() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::COOKIE,
            HeaderValue::from_static("theme=dark; lantor_web_session=abc123; other=value"),
        );
        assert_eq!(
            cookie_value(&headers, WEB_AUTH_COOKIE),
            Some("abc123".to_owned())
        );
    }
}
