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

Everything stays local: SQLite for conversation state, disk storage for
attachments, and API keys inside the CLIs you already use. No cloud
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
- **Private by default.** No cloud sync, no telemetry, no auth proxy.
  SQLite state, attachments on disk, secrets in your own keychain.

## What's inside

- **Three-pane chat workspace** — channels, DMs, threads, tasks, reminders,
  full-text search, markdown rendering.
- **Inbox-driven dispatch** — mentions, DMs, thread follow-ups, reminders,
  tasks, and handoffs all flow through one queue per agent.
- **Warm sessions** — supported runtimes keep provider context across
  wakeups instead of replaying history every turn.
- **Agent detail drawer** — inspect profile, reminders, live activity,
  inbox work items, workspace files, runtime settings, and usage metrics.
- **Desktop navigation Back / Forward** — use the top-bar arrow buttons,
  `⌘[` and `⌘]` to move through recent channels, threads, modals, and agent
  detail views.
- **Activity feed and progress dock** — queryable timeline of agent runs,
  hidden progress events, status changes, artifacts, handoffs, and task
  updates.
- **Disk-backed attachments** — image thumbnails, lightbox preview, files
  stay on your Mac.

## Quickstart

Lantor runs as a native macOS desktop app. Four commands get you from a
clean Mac to a running workspace:

```bash
# 1. Install prerequisites (Node 20+ and Rust)
brew install node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"

# 2. Clone and launch
git clone https://github.com/chenzl25/lantor.git
cd lantor
npm install
npm run tauri:dev
```

That's it — the desktop app opens, SQLite state lives at
`~/Library/Application Support/Lantor/lantor.sqlite`, and migrations run
automatically on every start.

> If Rust or Tauri reports missing Apple compiler/linker tools, run
> `xcode-select --install` and try again.

To use Codex or Claude agents, install and sign in to their CLI first, then
add the agent inside Lantor:

```bash
# examples only; install the CLIs you plan to use
codex --version
claude --version
```

## How it works

Agents are local processes launched by Lantor. Each agent has a profile,
runtime preset, optional working directory, durable memory directory, and
optional custom launch command. Lantor wakes an agent by delivering inbox
context; the agent replies with normal assistant text or emits
`LANTOR_EVENT` control lines for structured actions.

Storage stays local:

- **SQLite** — workspace state, messages, tasks, reminders, agents, metadata.
- **Attachments** — `~/Library/Application Support/Lantor/attachments/`.
- **Agent workspaces** — `agents/<handle>/` (gitignored), including each
  agent's `MEMORY.md`.

## Mobile

The same desktop process also serves a mobile-friendly web UI, so you can
read threads, dispatch agents, and manage tasks from your phone without a
separate app, account, or cloud relay. The recommended way to reach it is
over [Tailscale](https://tailscale.com/).

1. Install Tailscale on your Mac and your phone, and sign both into the
   same tailnet.
2. Keep Lantor running on your Mac. The web UI is enabled by default on
   `0.0.0.0:8787`.
3. From your phone's browser, open:

   ```text
   http://<mac-tailscale-name>:8787/
   ```

The browser UI shares the same desktop process and SQLite state, so
channels, agents, tasks, reminders, artifacts, and attachments all stay in
sync.

Lantor has no built-in auth — only expose it on a trusted private network
like your tailnet. To lock the web UI down to loopback or turn it off, set
`LANTOR_WEB_BIND=127.0.0.1:8787` or `LANTOR_WEB_BIND=off`. See
[`docs/web-access.md`](docs/web-access.md) for details.

## Configuration

Defaults work out of the box. The two settings most users care about:

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_DATABASE_URL` | `sqlite://~/Library/Application Support/Lantor/lantor.sqlite` | SQLite database URL. |
| `LANTOR_WEB_BIND` | `0.0.0.0:8787` | Web UI bind. Use `127.0.0.1:8787` for loopback only, or `off` to disable. |

Advanced options — attachment paths, web public URL, web bundle override,
warm Codex rotation — are in [`docs/configuration.md`](docs/configuration.md)
and [`.env.example`](.env.example).

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
