<p align="center">
  <img src="docs/assets/lantor-banner.png" alt="Lantor - AI Agent Workspace" width="820" />
</p>

# Lantor

> Local-first desktop workspace for one human and a team of local AI agents.

Lantor is a macOS app for coordinating local AI agents through channels, DMs,
threads, tasks, reminders, artifacts, and file attachments. There is no cloud:
conversation state lives in a local PostgreSQL, attachments live on disk, and
agents run as local CLIs (Codex, Claude, or any custom command) supervised
by the desktop app.

It is built for multi-agent work: agents can share one inbox, claim tasks,
handoff work to each other, keep durable memory on disk, and report progress
without sending your workspace state through a hosted coordination service.

> Status: early developer preview. Local-first, private, macOS only.

## Highlights

- **Multi-agent collaboration** — mentions, DMs, task handoff, competitive task claiming, and thread handoff route work to the right agent.
- **Persistent agent memory** — each agent owns a gitignored workspace with `MEMORY.md`, `notes/`, artifacts, and task files.
- **Three-pane chat workspace** — channels, DMs, threads, tasks, reminders, full-text search, and markdown rendering.
- **Agent detail drawer** — inspect profile, reminders, live activity, inbox work items, workspace files, runtime settings, and usage metrics.
- **Local agent supervision** — Codex, Claude, or any custom command, each with its own profile, avatar, runtime preset, and live edit preview.
- **Warm sessions** — supported runtimes keep provider context across wakeups instead of replaying history every turn.
- **Inbox-driven dispatch** — mentions, DMs, thread follow-ups, reminders, tasks, and handoffs all flow through one queue per agent.
- **Structured side effects** — agents emit `LANTOR_EVENT` control lines for activity, attachments, reminders, tasks, task claims, handoffs, profile changes, and more.
- **Disk-backed attachments** — image thumbnails, lightbox preview, files stay on your Mac.
- **Activity feed and progress dock** — queryable timeline of agent runs, hidden progress events, status changes, artifacts, handoffs, and task updates.
- **Mobile-ready browser access** — same desktop process exposes a web UI on `0.0.0.0:8787` so you can manage channels, agents, tasks, reminders, and threads from your phone over Tailscale.

## Requirements

- macOS
- Node.js 20+
- Rust toolchain with [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)
- PostgreSQL 14+ (local install or container)
- Optional agent CLIs installed and authenticated locally, such as `codex` and `claude`.

## Quickstart

```bash
npm install
psql postgres -c "create role lantor login password 'lantor';"
psql postgres -c "create database lantor owner lantor;"
npm run tauri:dev
```

Schema setup and migrations run automatically when the app starts. The desktop
app opens, and the same process serves the web UI at
`http://127.0.0.1:8787/` (and `http://<your-mac>:8787/` over Tailscale).

### Configuration

Everything is optional — defaults work for local development.

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_DATABASE_URL` | `postgres://lantor:lantor@127.0.0.1:5432/lantor` | Postgres connection string. `DATABASE_URL` is honored as a fallback. |
| `LANTOR_ATTACHMENT_DIR` | `~/Library/Application Support/Lantor/attachments` | Disk location for copied message attachments. |
| `LANTOR_WEB_BIND` | `0.0.0.0:8787` | Web UI bind. Set to `127.0.0.1:8787` for loopback only, or `off` to disable. |
| `LANTOR_WEB_PUBLIC_URL` | derived from bind | Public base URL used when generating web links. |
| `LANTOR_WEB_DIST` | auto-detected `dist/` | Override static web bundle directory for the browser UI. |
| `LANTOR_CODEX_CONTEXT_ROTATE_INPUT_TOKENS` | `180000` | Rotate warm Codex sessions after the latest stopped run exceeds this input-token count. Values below `50000` are ignored. |

See [`.env.example`](.env.example) for the full list.

## How It Works

Agents are local processes launched by Lantor. Each agent has a profile,
runtime preset, optional working directory, durable memory directory, runtime
preferences, and optional custom launch command.
Lantor wakes an agent by delivering inbox context; the agent replies with
normal assistant text or emits `LANTOR_EVENT` control lines for structured
actions.

Storage stays local:

- **PostgreSQL** — workspace state, messages, tasks, reminders, agents, metadata.
- **Attachments** — `~/Library/Application Support/Lantor/attachments/`.
- **Agent workspaces** — `agents/<handle>/` (gitignored), including each agent's `MEMORY.md`.

## Web UI / Tailscale Access

The web UI is enabled by default on `0.0.0.0:8787`. From another device on
your tailnet:

```text
http://<mac-tailscale-name>:8787/
```

It does **not** perform its own auth — only expose Lantor on a trusted
private network. See [`docs/web-access.md`](docs/web-access.md) for details
and how to lock it down to loopback.

The browser UI shares the same desktop process and PostgreSQL state. It covers
channels, threads, agents, tasks, reminders, artifacts, workspace previews, and
attachment previews so the phone workflow can manage the same agent workspace.

## Documentation

- [Agent runtime model](docs/agent-runtime.md)
- [Control events](docs/control-events.md)
- [Tailscale web access](docs/web-access.md)
- [Agent activity feed](docs/activity-feed.md)

Bug reports and feature requests are welcome via
[GitHub Issues](https://github.com/chenzl25/lantor/issues).

## Development

```bash
npm run build                                              # frontend bundle
cargo check --manifest-path src-tauri/Cargo.toml           # rust typecheck
cargo test  --manifest-path src-tauri/Cargo.toml --no-run  # compile tests
npm run tauri:dev                                          # desktop app
```

## License

MIT
