use std::{
    env,
    net::{IpAddr, SocketAddr},
    time::Instant,
};

use sqlx::SqlitePool;

use crate::{
    activity_store::{
        load_agent_activities, load_agent_activity_summaries, load_agent_run_summaries,
        load_agent_runs, load_agent_work_items,
    },
    agent_profile::{load_agents, load_owner_profile},
    app::CommandResult,
    channels::{load_channel_members, load_channels, load_thread_activities},
    domain::{reminders::load_reminders, schedules::load_agent_schedules},
    launch_agent,
    message_store::{
        load_artifacts, load_messages, load_recent_messages_per_channel, load_saved_messages,
    },
    models::{
        Bootstrap, BootstrapPerf, BootstrapPerfCounts, BootstrapPerfOptions, BootstrapPerfPhase,
    },
    owner_inbox::{load_dismissed_inbox_items, load_read_inbox_items},
    runtime::supervisor::load_supervisor_status,
    task_store::load_tasks,
    web,
};

const WEB_BOOTSTRAP_MESSAGES_PER_CHANNEL: i64 = 80;

fn configured_web_base_url() -> Option<String> {
    if let Ok(value) = env::var("LANTOR_WEB_PUBLIC_URL") {
        let trimmed = value.trim().trim_end_matches('/').to_owned();
        if !trimmed.is_empty() {
            return Some(trimmed);
        }
    }
    let bind = web::resolve_web_bind()?;
    let addr = bind.parse::<SocketAddr>().ok()?;
    let host = match addr.ip() {
        IpAddr::V4(ip) if ip.is_unspecified() => "127.0.0.1".to_owned(),
        IpAddr::V4(ip) => ip.to_string(),
        IpAddr::V6(ip) if ip.is_unspecified() => "[::1]".to_owned(),
        IpAddr::V6(ip) => format!("[{ip}]"),
    };
    Some(format!("http://{host}:{}", addr.port()))
}

pub(crate) async fn load_bootstrap(pool: &SqlitePool, db_url: String) -> CommandResult<Bootstrap> {
    load_bootstrap_with_options(
        pool,
        db_url,
        BootstrapLoadOptions {
            runtime: "tauri",
            messages: BootstrapMessageLoad::All,
            include_run_logs: true,
            compact_agent_activities: false,
        },
    )
    .await
}

pub(crate) async fn load_web_bootstrap(
    pool: &SqlitePool,
    db_url: String,
) -> CommandResult<Bootstrap> {
    load_bootstrap_with_options(
        pool,
        db_url,
        BootstrapLoadOptions {
            runtime: "web",
            messages: BootstrapMessageLoad::RecentPerChannel(WEB_BOOTSTRAP_MESSAGES_PER_CHANNEL),
            include_run_logs: false,
            compact_agent_activities: true,
        },
    )
    .await
}

struct BootstrapLoadOptions {
    runtime: &'static str,
    messages: BootstrapMessageLoad,
    include_run_logs: bool,
    compact_agent_activities: bool,
}

#[derive(Clone, Copy)]
enum BootstrapMessageLoad {
    All,
    RecentPerChannel(i64),
}

impl BootstrapMessageLoad {
    fn label(self) -> String {
        match self {
            BootstrapMessageLoad::All => "All".to_owned(),
            BootstrapMessageLoad::RecentPerChannel(limit) => format!("RecentPerChannel({limit})"),
        }
    }
}

fn elapsed_ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

fn push_phase(
    phases: &mut Vec<BootstrapPerfPhase>,
    name: &str,
    started_at: Instant,
    rows: Option<usize>,
) {
    phases.push(BootstrapPerfPhase {
        name: name.to_owned(),
        duration_ms: elapsed_ms(started_at),
        rows,
    });
}

fn should_measure_bootstrap_payload() -> bool {
    env::var("LANTOR_PERF").is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
        || env::var("LANTOR_PERF_PAYLOAD")
            .is_ok_and(|value| value == "1" || value.eq_ignore_ascii_case("true"))
}

async fn load_bootstrap_with_options(
    pool: &SqlitePool,
    db_url: String,
    options: BootstrapLoadOptions,
) -> CommandResult<Bootstrap> {
    let total_started_at = Instant::now();
    let mut phases = Vec::new();

    let started_at = Instant::now();
    let owner_profile = load_owner_profile(pool).await?;
    push_phase(&mut phases, "owner_profile", started_at, Some(1));

    let started_at = Instant::now();
    let channels = load_channels(pool).await?;
    push_phase(&mut phases, "channels", started_at, Some(channels.len()));

    let started_at = Instant::now();
    let thread_activities = load_thread_activities(pool).await?;
    push_phase(
        &mut phases,
        "thread_activities",
        started_at,
        Some(thread_activities.len()),
    );

    let started_at = Instant::now();
    let channel_members = load_channel_members(pool).await?;
    push_phase(
        &mut phases,
        "channel_members",
        started_at,
        Some(channel_members.len()),
    );

    let started_at = Instant::now();
    let agents = load_agents(pool).await?;
    push_phase(&mut phases, "agents", started_at, Some(agents.len()));

    let started_at = Instant::now();
    let messages = match options.messages {
        BootstrapMessageLoad::All => load_messages(pool).await?,
        BootstrapMessageLoad::RecentPerChannel(limit) => {
            load_recent_messages_per_channel(pool, limit).await?
        }
    };
    push_phase(&mut phases, "messages", started_at, Some(messages.len()));

    let started_at = Instant::now();
    let saved_messages = load_saved_messages(pool).await?;
    push_phase(
        &mut phases,
        "saved_messages",
        started_at,
        Some(saved_messages.len()),
    );

    let started_at = Instant::now();
    let dismissed_inbox_items = load_dismissed_inbox_items(pool).await?;
    push_phase(
        &mut phases,
        "dismissed_inbox_items",
        started_at,
        Some(dismissed_inbox_items.len()),
    );

    let started_at = Instant::now();
    let read_inbox_items = load_read_inbox_items(pool).await?;
    push_phase(
        &mut phases,
        "read_inbox_items",
        started_at,
        Some(read_inbox_items.len()),
    );

    let started_at = Instant::now();
    let artifacts = load_artifacts(pool).await?;
    push_phase(&mut phases, "artifacts", started_at, Some(artifacts.len()));

    let started_at = Instant::now();
    let tasks = load_tasks(pool).await?;
    push_phase(&mut phases, "tasks", started_at, Some(tasks.len()));

    let started_at = Instant::now();
    let reminders = load_reminders(pool).await?;
    push_phase(&mut phases, "reminders", started_at, Some(reminders.len()));

    let started_at = Instant::now();
    let agent_schedules = load_agent_schedules(pool).await?;
    push_phase(
        &mut phases,
        "agent_schedules",
        started_at,
        Some(agent_schedules.len()),
    );

    let started_at = Instant::now();
    let agent_runs = if options.include_run_logs {
        load_agent_runs(pool).await?
    } else {
        load_agent_run_summaries(pool).await?
    };
    push_phase(
        &mut phases,
        "agent_runs",
        started_at,
        Some(agent_runs.len()),
    );

    let started_at = Instant::now();
    let agent_work_items = load_agent_work_items(pool).await?;
    push_phase(
        &mut phases,
        "agent_work_items",
        started_at,
        Some(agent_work_items.len()),
    );

    let started_at = Instant::now();
    let agent_activities = if options.compact_agent_activities {
        load_agent_activity_summaries(pool).await?
    } else {
        load_agent_activities(pool).await?
    };
    push_phase(
        &mut phases,
        "agent_activities",
        started_at,
        Some(agent_activities.len()),
    );

    let started_at = Instant::now();
    let supervisor = load_supervisor_status(pool).await?;
    push_phase(&mut phases, "supervisor", started_at, None);

    let started_at = Instant::now();
    let launch_agent = launch_agent::load_launch_agent_status()?;
    push_phase(&mut phases, "launch_agent", started_at, None);

    let counts = BootstrapPerfCounts {
        channels: channels.len(),
        thread_activities: thread_activities.len(),
        channel_members: channel_members.len(),
        agents: agents.len(),
        messages: messages.len(),
        saved_messages: saved_messages.len(),
        dismissed_inbox_items: dismissed_inbox_items.len(),
        read_inbox_items: read_inbox_items.len(),
        artifacts: artifacts.len(),
        tasks: tasks.len(),
        reminders: reminders.len(),
        agent_schedules: agent_schedules.len(),
        agent_runs: agent_runs.len(),
        agent_work_items: agent_work_items.len(),
        agent_activities: agent_activities.len(),
    };

    let mut bootstrap = Bootstrap {
        db_url,
        web_base_url: configured_web_base_url(),
        owner_profile,
        channels,
        thread_activities,
        channel_members,
        agents,
        messages,
        saved_messages,
        dismissed_inbox_items,
        read_inbox_items,
        artifacts,
        tasks,
        reminders,
        agent_schedules,
        agent_runs,
        agent_work_items,
        agent_activities,
        supervisor,
        launch_agent,
        perf: None,
    };

    let (serialize_ms, payload_bytes) = if should_measure_bootstrap_payload() {
        let serialize_started_at = Instant::now();
        let payload_bytes = serde_json::to_vec(&bootstrap)
            .map(|payload| payload.len())
            .ok();
        (Some(elapsed_ms(serialize_started_at)), payload_bytes)
    } else {
        (None, None)
    };
    bootstrap.perf = Some(BootstrapPerf {
        options: BootstrapPerfOptions {
            runtime: options.runtime.to_owned(),
            message_load: options.messages.label(),
            include_run_logs: options.include_run_logs,
            compact_agent_activities: options.compact_agent_activities,
        },
        total_ms: elapsed_ms(total_started_at),
        serialize_ms,
        payload_bytes,
        phases,
        counts,
    });

    Ok(bootstrap)
}
