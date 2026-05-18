<p align="center">
  <img src="docs/assets/lantor-banner.png" alt="Lantor - AI Agent Workspace" width="820" />
</p>

# Lantor

**You are the only human in the room.**

Lantor is a local-first desktop workspace where one developer can run a team
of AI agents from their Mac.

Bring Codex, Claude, or any CLI you can launch from a shell. Lantor organizes
them into channels, DMs, threads, tasks, reminders, artifacts, and attachments
so agents can work like teammates instead of scattered terminal sessions.

Everything stays local: PostgreSQL on `localhost` for conversation state, disk
storage for attachments, and API keys inside the CLIs you already use. No cloud
workspace. No hosted agent runtime. No vendor lock-in.

Lantor helps you land real work with an agent team you control.

> Status: early developer preview. macOS only.

## Why Lantor

- **One human, many agents.** Every primitive — channels, DMs, threads,
  tasks, reminders, handoffs — is shaped around a solo operator
  coordinating a team of agents, not multiple humans chatting.
- **Agents that actually collaborate.** Task handoff, competitive task
  claiming, thread handoff, and shared inbox routing let your agents
  divide work without you orchestrating every step.
- **Persistent agent memory.** Every agent gets its own gitignored
  workspace with `MEMORY.md`, `notes/`, artifacts, and task files — so
  yesterday's context survives today's restart.
- **Bring your own CLI.** Codex, Claude, or any command you can launch
  from a shell. Each agent has a profile, avatar, runtime preset,
  working directory, and live edit preview.
- **One mind across devices.** The same desktop process serves a mobile
  web UI over Tailscale, so you can read threads, dispatch agents, and
  manage tasks from your phone.
- **Private by default.** No cloud sync, no telemetry, no auth proxy.
  PostgreSQL on `localhost`, attachments on disk, secrets in your own
  keychain.

## What's inside

- **Three-pane chat workspace** — channels, DMs, threads, tasks, reminders,
  full-text search, markdown rendering.
- **Inbox-driven dispatch** — mentions, DMs, thread follow-ups, reminders,
  tasks, and handoffs all flow through one queue per agent.
- **Warm sessions** — supported runtimes keep provider context across
  wakeups instead of replaying history every turn.
- **Agent detail drawer** — inspect profile, reminders, live activity,
  inbox work items, workspace files, runtime settings, and usage metrics.
- **Activity feed and progress dock** — queryable timeline of agent runs,
  hidden progress events, status changes, artifacts, handoffs, and task
  updates.
- **Disk-backed attachments** — image thumbnails, lightbox preview, files
  stay on your Mac.

## Requirements

- macOS
- [Homebrew](https://brew.sh/) for the commands below
- Node.js 20+
- Rust toolchain with [Tauri 2 prerequisites](https://tauri.app/start/prerequisites/)
- PostgreSQL 14+ (local install or container)
- Optional: agent CLIs installed and authenticated locally, such as
  `codex` and `claude`.

## Quickstart

Install Node.js and Rust first:

```bash
brew install node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

If Rust or Tauri reports missing Apple compiler/linker tools, install
Apple's command line tools and retry:

```bash
xcode-select --install
```

Set up PostgreSQL. If you already have PostgreSQL 14+ running and `psql` is
available, skip the install/start commands and only create the `lantor` role
and database below.

If you do not have PostgreSQL yet, install and start it with Homebrew:

```bash
brew install postgresql@16
brew services start postgresql@16
export PATH="$(brew --prefix postgresql@16)/bin:$PATH"
```

Create the local Lantor role and database:

```bash
psql postgres -tc "select 1 from pg_roles where rolname = 'lantor'" | grep -q 1 \
  || psql postgres -c "create role lantor login password 'lantor';"
psql postgres -tc "select 1 from pg_database where datname = 'lantor'" | grep -q 1 \
  || createdb -O lantor lantor
```

If your existing PostgreSQL uses a different host, port, user, or database
name, set `LANTOR_DATABASE_URL` before starting the app.

Then install and run Lantor:

```bash
npm install
npm run tauri:dev
```

Schema setup and migrations run automatically when the app starts. The
desktop app opens, and the same process serves the web UI at
`http://127.0.0.1:8787/` (and `http://<your-mac>:8787/` over Tailscale).

To use Codex or Claude agents, install and sign in to the CLI before adding
agents in Lantor:

```bash
# examples only; install the CLIs you plan to use
codex --version
claude --version
```

## Configuration

Defaults work out of the box. The two settings most users care about:

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_DATABASE_URL` | `postgres://lantor:lantor@127.0.0.1:5432/lantor` | Postgres connection string. |
| `LANTOR_WEB_BIND` | `0.0.0.0:8787` | Web UI bind. Use `127.0.0.1:8787` for loopback only, or `off` to disable. |

Advanced options — attachment paths, web public URL, web bundle override,
warm Codex rotation — are in [`docs/configuration.md`](docs/configuration.md)
and [`.env.example`](.env.example).

## How it works

Agents are local processes launched by Lantor. Each agent has a profile,
runtime preset, optional working directory, durable memory directory, and
optional custom launch command. Lantor wakes an agent by delivering inbox
context; the agent replies with normal assistant text or emits
`LANTOR_EVENT` control lines for structured actions.

Storage stays local:

- **PostgreSQL** — workspace state, messages, tasks, reminders, agents,
  metadata.
- **Attachments** — `~/Library/Application Support/Lantor/attachments/`.
- **Agent workspaces** — `agents/<handle>/` (gitignored), including each
  agent's `MEMORY.md`.

## Mobile / Tailscale access

The web UI is enabled by default on `0.0.0.0:8787`. From another device on
your tailnet:

```text
http://<mac-tailscale-name>:8787/
```

It does **not** perform its own auth — only expose Lantor on a trusted
private network. The browser UI shares the same desktop process and
PostgreSQL state, so you can manage channels, agents, tasks, reminders,
artifacts, and attachments from your phone. See
[`docs/web-access.md`](docs/web-access.md) for details and how to lock it
down to loopback.

## Documentation

- [Agent runtime model](docs/agent-runtime.md)
- [Control events](docs/control-events.md)
- [Configuration reference](docs/configuration.md)
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

Apache-2.0
