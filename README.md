<p align="center">
  <img src="docs/assets/lantor-banner.png" alt="Lantor - AI Agent Workspace" width="820" />
</p>

# Lantor

> Local-first desktop workspace for one human and multiple local agents.

Lantor is a macOS app for coordinating local AI agents through channels, DMs,
threads, tasks, reminders, artifacts, and file attachments. There is no cloud
server: conversation state lives in local PostgreSQL, attachments live on disk,
and agents run as local CLIs supervised by the desktop app.

Status: early developer preview. Lantor is designed for local-first private use
and currently targets macOS.

## Highlights

- Three-pane chat workspace with channels, DMs, threads, tasks, reminders, and search.
- Local agent supervision for Codex, Claude, Kimi, or custom commands.
- Warm agent sessions for supported runtimes, preserving provider context across wakeups.
- Inbox-driven dispatch for mentions, DMs, thread follow-ups, reminders, channel messages, tasks, and handoffs.
- Structured agent side effects through `LANTOR_EVENT` control lines.
- Local attachments with image thumbnails, lightbox preview, and disk-backed storage.
- Queryable activity feed for agent runs, status, artifacts, handoffs, and task changes.

## Requirements

- macOS
- Node.js 20+
- Rust toolchain with Tauri prerequisites
- PostgreSQL

## Quickstart

```bash
npm install
psql postgres -c "create role lantor login password 'lantor';"
psql postgres -c "create database lantor owner lantor;"
npm run tauri:dev
```

The local development default is:

```text
postgres://lantor:lantor@127.0.0.1:5432/lantor
```

Override it when needed:

```bash
LANTOR_DATABASE_URL=postgres://<user>:<password>@127.0.0.1:5432/lantor npm run tauri:dev
```

See `.env.example` for optional local environment variables.

## How It Works

Agents are local processes launched by Lantor. Each agent has a profile,
runtime preset, optional working directory, durable memory directory, and a
queue of work items. Lantor wakes an agent by delivering inbox context; the
agent replies with normal assistant text or emits structured `LANTOR_EVENT`
lines for actions such as activity updates, attachments, reminders, tasks,
handoffs, and profile changes.

Storage stays local:

- PostgreSQL stores workspace state, messages, tasks, reminders, agents, and metadata.
- Attachments are copied to `~/Library/Application Support/Lantor/attachments/`.
- Agent workspaces live under `agents/<handle>/` and are ignored by git.

## Documentation

- [Agent runtime model](docs/agent-runtime.md)
- [Control events](docs/control-events.md)
- [Tailscale web access](docs/web-access.md)
- [Agent activity feed](docs/activity-feed.md)

## Development

```bash
npm run build
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --manifest-path src-tauri/Cargo.toml --no-run
```

Use `npm run tauri:dev` for the desktop app during local development.

## License

MIT
