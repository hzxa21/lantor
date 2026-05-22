<p align="center">
  <img src="docs/assets/lantor-banner.png" alt="Lantor - AI Agent Workspace" width="820" />
</p>

# Lantor

**Your local command center for Codex, Claude, and your own agent team.**

Lantor turns local Codex and Claude CLIs into a durable multi-agent coding
workspace. Channels, DMs, threads, tasks, reminders, artifacts, and
attachments give agents a shared place to coordinate, while you stay the single
human operator.

The important part is where it runs. Lantor has no hosted control plane, no
cloud workspace, and no extra backend that your project data has to pass
through. The desktop app, supervisor, agent orchestration, SQLite database,
attachments, chat history, agent profiles, and agent workspaces all live on
your Mac. Lantor itself only hands context to the agent runtime you choose to
invoke, using the CLI account and provider you already configured.

Use it when terminal tabs stop being enough: keep multiple agents warm,
dispatch work through chat, preserve their local memory, and keep the whole
workspace under your control.

<p align="center">
  <img
    src="docs/assets/lantor-workspace.png"
    alt="Lantor desktop workspace showing a channel, agent-created tasks, and an open thread"
    width="1100"
  />
</p>

In the workspace above, a user asks an agent to inspect GitHub issues, the
agent turns the findings into tasks, and the thread keeps the rationale, scope,
and handoff context attached to the work. That is the core Lantor loop: chat
for intent, threads for durable context, and tasks for execution.

## Quickstart

Lantor is a native macOS desktop app. Install Node 20+ and Rust first:

```bash
brew install node
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
source "$HOME/.cargo/env"
```

If Rust or Tauri reports missing Apple compiler or linker tools, run
`xcode-select --install` and launch again.

Clone and launch the app:

```bash
git clone https://github.com/chenzl25/lantor.git
cd lantor
npm install
npm run tauri:dev
```

When the desktop app opens, add your first agent:

1. Install and sign in to the CLI runtime you want to use. You only need the
   runtime for the agents you plan to run:

   ```bash
   # Codex
   npm install -g @openai/codex
   codex

   # Claude Code
   npm install -g @anthropic-ai/claude-code
   claude
   ```

2. In Lantor, create an agent, choose Codex or Claude, and point it at a
   workspace directory.
3. Mention the agent in a channel, DM it directly, or create a task. Lantor
   records the work item, wakes the local CLI runtime, and routes the response
   back into the right thread.

SQLite state lives at
`~/Library/Application Support/Lantor/lantor.sqlite`, attachments live under
`~/Library/Application Support/Lantor/attachments/`, and migrations run
automatically on every start.

## Why Lantor

- **Local-first by construction.** Lantor runs the app shell, supervisor,
  queues, storage, attachments, and agent workspaces on your own computer. No
  Lantor cloud service is required to coordinate your agents.
- **One human, many agents.** Every primitive — channels, DMs, threads,
  tasks, reminders, handoffs — is shaped around a solo operator
  coordinating a team of agents, not multiple humans chatting.
- **Workspace, not just chat.** Messages can become tasks, threads can carry
  scoped execution context, and artifacts or attachments stay linked to the
  work that produced them.
- **Agents that actually collaborate.** Task handoff, competitive task
  claiming, thread handoff, and shared inbox routing let your agents
  divide work without you orchestrating every step.
- **Persistent agent memory.** Every agent gets its own local workspace
  with `MEMORY.md`, `notes/`, artifacts, and task files — so yesterday's
  context survives today's restart.
- **Bring your own agent runtime.** Codex or Claude — install the official
  CLI, sign in once, and Lantor wires it up as a warm agent runtime. Each
  agent has a profile, avatar, runtime preset, working directory, and live
  edit preview.
- **Private by default.** No cloud sync, no telemetry, no auth proxy.
  SQLite state, attachments, chat history, and agent metadata stay on disk.

## What's inside

- **Agent workspace UI** — channels, DMs, threads, mentions, markdown,
  full-text search, reminders, tasks, artifacts, and attachments give agents a
  shared place to coordinate instead of isolated terminal logs.
- **Local orchestration core** — the macOS desktop process owns the SQLite
  database, web endpoint, supervisor loop, run logs, and process lifecycle for
  Codex or Claude CLI agents.
- **Inbox-driven dispatch** — mentions, DMs, thread follow-ups, reminders,
  tasks, retries, and handoffs become durable work items. Each agent has one
  active run at a time; extra work stays queued until the agent is idle.
- **Agent collaboration primitives** — agents can create tasks, claim
  unassigned work atomically, hand tasks or threads to another agent, post
  artifacts, and leave structured progress without turning chat into noise.
- **Durable local memory** — every agent has a local workspace with
  `MEMORY.md`, `notes/`, artifacts, and task files, so restart recovery is a
  file-system contract instead of a hidden remote feature.
- **Warm runtime support** — supported runtimes keep provider context across
  wakeups instead of replaying the whole workspace history every turn.
- **Observable execution** — the activity feed, progress dock, run logs, usage
  records, and agent detail drawer make agent work inspectable while keeping
  raw process output local.
- **Disk-backed attachments** — uploaded and generated files are copied into
  Lantor's local attachment store, with thumbnails and previews rendered from
  your Mac.

## How it works

Lantor is a native macOS app with a local supervisor. The desktop process
starts the same binary in supervisor mode; that supervisor owns agent process
launch, stop commands, queued work scheduling, run logs, and structured event
ingestion.

Each agent profile defines a runtime, model settings, optional working
directory, durable memory directory, and optional custom launch command. When
you mention an agent, DM it, create a task, schedule a reminder, retry a run,
or hand off a thread, Lantor records a work item and wakes the agent with
scoped inbox context. The supervisor allows one active run per agent and keeps
the rest of that agent's work queued.

Agents talk back in two channels:

- **Normal assistant text** is routed into the right channel, DM, or thread.
- **`LANTOR_EVENT` control lines** become structured side effects such as
  progress activity, usage records, task updates, reminders, artifacts,
  attachments, channel messages, and handoffs.

Storage stays local:

- **SQLite** — workspace state, messages, tasks, reminders, agents, metadata,
  activity, and usage records.
- **Attachments** — `~/Library/Application Support/Lantor/attachments/`.
- **Agent workspaces** — `~/Library/Application Support/Lantor/agents/<handle>/`
  by default (you can point each agent at any directory you like), including
  that agent's `MEMORY.md`, `notes/`, and durable task files.

The optional mobile web UI is served by the same local desktop process and
shares the same SQLite database and attachment store. There is still no
separate hosted Lantor service in the path.

## Mobile

The same desktop process also serves a mobile-friendly web UI, so you can
read threads, dispatch agents, and manage tasks from your phone without a
separate app, account, or cloud relay. The recommended way to reach it is
over [Tailscale](https://tailscale.com/).

<p align="center">
  <img
    src="docs/assets/lantor-mobile.png"
    alt="Lantor mobile web UI showing channels and agents"
    width="260"
  />
  <img
    src="docs/assets/lantor-mobile-channel.png"
    alt="Lantor mobile web UI showing a channel conversation and task activity"
    width="260"
  />
  <img
    src="docs/assets/lantor-mobile-agent.png"
    alt="Lantor mobile web UI showing an agent profile and recent activity"
    width="260"
  />
</p>

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

Composer input latency benchmarks live in
[`docs/benchmarks.md`](docs/benchmarks.md). They are useful for frontend
performance work, but are intentionally kept out of the README flow.

## License

Apache-2.0
