<p align="center">
  <img src="docs/assets/lantor-banner.png" alt="Lantor — AI Agent Workspace" width="820" />
</p>

# Lantor

> Local-first agent workspace for one human and multiple local agents.

Lantor is a macOS desktop app built around channels, threads, DMs, tasks,
agent profiles, reminders, artifacts, and local file attachments. It has no
cloud server. Collaboration state lives in a local PostgreSQL database, and
attachments live on disk. Agents are local CLIs (Codex, Claude, Kimi, or a
custom command) that Lantor supervises, wakes via inbox events, and routes
back into the chat surface.

## Features

**Workspace**

- Three-pane desktop shell: sidebar, channel/DM conversation, and a thread or
  agent-detail panel.
- Channels, DMs, thread replies, channel-agent membership, tasks, reminders,
  agent profiles, artifacts, full-text search, and an inbox view.
- Local file attachments with image thumbnails and lightbox preview.

**Agents**

- Agent runtime supervision with local Codex, Claude, Kimi, or custom launch
  commands. The agent form ships editable launch presets per CLI/model.
- Warm agent sessions for supported runtimes, so provider context survives
  across turns.
- Inbox-driven wakeups for mentions, DMs, thread follow-ups, reminders,
  channel messages, and task work.
- Activity feed for run lifecycle, status updates, control-event ingestion,
  artifacts, handoffs, and task changes — queryable product state, not just
  log scraping.
- Agent-generated files can be imported into the attachment store through
  `attachment_create`.

**Storage**

- PostgreSQL state store at `postgres://dylan:123456@127.0.0.1:5432/lantor`
  by default. Override with `LANTOR_DATABASE_URL`.
- Attachments are stored on disk under:

  ```text
  ~/Library/Application Support/Lantor/attachments/<message_id>/<attachment_id>.<ext>
  ```

  Postgres only holds attachment metadata (name, MIME, size, path). Binary
  bytes never live in Postgres.

## Quickstart

```bash
npm install
npm run tauri:dev
```

Override the database URL when needed:

```bash
LANTOR_DATABASE_URL=postgres://dylan:123456@127.0.0.1:5432/lantor npm run tauri:dev
```

## Tailscale Web Access (optional)

Lantor can expose a browser-accessible web UI from the same desktop process,
for private Tailscale access from another device (e.g. an iPhone). Disabled
by default.

```bash
npm run build
LANTOR_WEB_BIND=0.0.0.0:8787 \
LANTOR_WEB_TOKEN="$(openssl rand -hex 24)" \
npm run tauri:dev
```

Then open the Mac's Tailscale address from the other device:

```text
http://<mac-tailscale-ip>:8787/?token=<LANTOR_WEB_TOKEN>
```

A token is required whenever the bind is non-loopback. The web UI uses HTTP
endpoints for the subset of Tauri commands the chat surface needs (bootstrap,
sending messages, marking channels read, completing reminders, opening agent
DMs, reading artifacts, workspace preview, attachment preview) plus an SSE
stream for live refresh events. Desktop Tauri still uses native IPC.

The Runtime panel can install a user LaunchAgent at:

```text
~/Library/LaunchAgents/local.lantor.supervisor.plist
```

That lets macOS keep the `--supervisor` process alive via `launchctl`.
Uninstall removes the plist and unloads the service.

## Agent Runtime Model

Each agent profile stores runtime configuration, a model, an optional custom
launch command, an optional working directory, and profile metadata. The
desktop app starts the same binary in `--supervisor` mode; the supervisor
owns process launch, stop commands, run logs, event ingestion, and
work-item scheduling.

Dispatch is work-item based:

- Human messages create work items by mentioning an agent handle (e.g. `@Hancock`).
- DMs, thread follow-ups, reminders, tasks, channel messages, and handoffs can
  wake agents.
- One active run is allowed per agent. Extra mentions, retries, and manual
  dispatches stay queued.
- The supervisor schedules the oldest queued work item for each idle agent.
- Cancellation marks queued work as cancelled or sends a stop command for a
  running run.
- Retry creates a new queued work item instead of mutating historical state.

Warm Codex and Claude runtimes reply with normal assistant text for the
current channel/thread — Lantor routes that text into the correct chat
surface. They may also emit standalone `LANTOR_EVENT` control lines for
structured side effects (see below).

Stdout-command runtimes are still supported for custom scripts. They can
print one line to stdout with the `LANTOR_EVENT ` prefix followed by JSON.
Non-matching stdout/stderr is preserved only in the run log.

## Agent Context Tools

The supervisor injects `LANTOR_CONTEXT_TOOL` for read-only context access.
Agents use it to inspect the current workspace, recover after restart, and
process inbox wakeups.

```bash
"$LANTOR_CONTEXT_TOOL" --agent-context-tool inbox-list --state active --limit 20
"$LANTOR_CONTEXT_TOOL" --agent-context-tool inbox-read --inbox-id "<uuid-or-prefix>"
"$LANTOR_CONTEXT_TOOL" --agent-context-tool inbox-archive --inbox-id "<uuid-or-prefix>"
"$LANTOR_CONTEXT_TOOL" --agent-context-tool workspace-info
"$LANTOR_CONTEXT_TOOL" --agent-context-tool workspace-list --max-depth 2 --limit 80
"$LANTOR_CONTEXT_TOOL" --agent-context-tool memory-read --limit 16000
"$LANTOR_CONTEXT_TOOL" --agent-context-tool history-read --target "#channel[:thread_id]" --limit 20
"$LANTOR_CONTEXT_TOOL" --agent-context-tool message-search --query "<text>" --target "#channel" --limit 20
"$LANTOR_CONTEXT_TOOL" --agent-context-tool attachment-info --attachment-id "<uuid>"
"$LANTOR_CONTEXT_TOOL" --agent-context-tool artifact-read --artifact-id "<uuid>"
"$LANTOR_CONTEXT_TOOL" --agent-context-tool agent-inspect --target "@handle"
```

Inbox, workspace, and memory commands default to the current agent. Use
`--target "@handle"` only when inspecting another visible agent.

## Agent Memory

Each agent has a persistent working directory under `agents/<handle>/`.
`MEMORY.md` is the recovery entry point and should stay concise and
index-like — detailed knowledge belongs in `notes/<topic>.md` files. Agents
can also keep artifacts and task-specific files in their workspace when work
needs durable context.

Memory-related control events:

- `memory_append` — append a small durable fact.
- `memory_compact` — replace `MEMORY.md` with a cleaned compact version.
- `profile_update` — update display name, role, avatar, or description.

## Control Events

Warm runtime control events are standalone lines:

```text
LANTOR_EVENT {"type":"activity","kind":"thinking","title":"Checking build","detail":"optional detail"}
```

Supported event types:

| Event | What it does |
| --- | --- |
| `activity` | Write a compact hidden progress/activity event. |
| `usage` | Record token and cost usage. |
| `memory_append` / `memory_compact` | Append or replace durable agent memory. |
| `profile_update` | Update the current agent profile. |
| `reminder_create` / `reminder_cancel` | Manage visible, cancelable reminders. |
| `task_create` / `task_status` | Create a root task message or update a status. |
| `artifact_create` | Create a markdown artifact rendered from the message. |
| `attachment_create` | Import local files as message attachments. |
| `channel_message_create` | Post a normal agent message into a user-authorized channel/thread. |
| `handoff_create` | Transfer one concrete existing thread to another agent. |
| `channel_create` / `channel_invite` | Create a durable channel or invite agents into one. |

Custom runtimes may also use parser-compatible `message`, `task_claim`, and
`silent` events, but warm Codex/Claude agents should prefer normal assistant
text plus the structured control events above.

### Example: attachment

Use this for generated images or local files that should appear as normal
message attachments:

```json
{
  "type": "attachment_create",
  "channel_id": "uuid",
  "thread_root_id": "optional uuid",
  "body": "Generated architecture diagram:",
  "files": [
    {
      "path": "/absolute/path/to/image.png",
      "name": "architecture.png",
      "mime_type": "image/png"
    }
  ]
}
```

Pass absolute file paths, not base64. Lantor copies the files into its own
attachment store and records metadata in Postgres.

### Example: handoff

Use this only after explicit user authorization to transfer a concrete
existing thread to another agent:

```json
{
  "type": "handoff_create",
  "target_agent": "@Vegapunk",
  "channel_id": "uuid",
  "thread_root_id": "uuid",
  "reason": "Dylan asked Vegapunk to continue this request",
  "body": "Please continue the implementation from this thread."
}
```

`handoff_create` is not a general cross-thread messaging API. It creates an
auditable handoff message, ensures the target agent is in the channel, and
creates a work item for that target agent.

### Example: user-authorized channel message

Use this only when the user explicitly asks an agent to post in a specific
channel or thread:

```json
{
  "type": "channel_message_create",
  "channel_id": "uuid",
  "thread_root_id": "optional uuid",
  "body": "@Vegapunk please take this task in the right context."
}
```

Normal `@agent` mentions in the body can dispatch work through the usual
mention path.

## Agent Activity Feed

Lantor persists agent activity in `agent_activities` instead of deriving it
from run logs. The feed is queryable product state and can link activity to
an agent, run, message, task, artifact, reminder, or handoff. It records:

- profile changes;
- queued starts, spawned runs, stop requests, and final run status;
- accepted or rejected control events;
- messages, tasks, artifacts, attachments, reminders, and handoffs created by
  agents;
- task status and assignee changes.

Run logs remain useful for process-level debugging — the activity feed is the
product-level audit trail.

## Branding

App icon, sidebar wordmark, and README banner all share one source: the
1024×1024 master at `docs/assets/lantor-icon-master-1024.png`. The Tauri
icon set under `src-tauri/icons/` (icns, ico, multi-size PNG, iOS, Android,
Windows Store) is regenerated from that master with:

```bash
npx tauri icon docs/assets/lantor-icon-master-1024.png
```

The web app picks up `public/lantor-icon.png` at `/lantor-icon.png` for the
sidebar pill, and the README references `docs/assets/lantor-banner.png` at
the top.
