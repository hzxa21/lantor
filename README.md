# LocalSlock

Local-only Slock-style desktop console for one human and multiple local agents.

## First Version Scope

- macOS desktop shell with a three-pane layout: channels, chat, thread/task context.
- PostgreSQL state store at `postgres://dylan:123456@127.0.0.1:5432/localslock`.
- Clean initialization: the app creates schema only and does not seed demo data.
- UI operations for channels, messages, thread replies, channel-scoped tasks, and agent profiles.
- Agent runtime runs through a local `--supervisor` mode with start/stop controls, process status, and persisted run logs in Postgres.
- Agent activity feed records profile changes, run lifecycle changes, stdout event ingestion, message creation, and task changes as durable Postgres state.
- Single Apple-style Liquid Glass visual direction.
- No cloud server, no multi-human permissions, no web deployment.

This version establishes the local product shell and data model first. It intentionally keeps collaboration semantics local-only: tasks are top-level channel messages, each task has a thread, the task board can update title, assignee, and status, and read/follow state is persisted in Postgres.

## Iteration Path

1. MVP operations: create/edit/delete channels and agents, root messages, thread replies, channel task creation, task title/assignee/status updates.
2. Agent runtime: launch/stop local Codex, Claude, and Kimi processes with logs and status.
3. Collaboration semantics: local search, unread state, thread follow/unfollow, channel membership, and local notifications.
4. Desktop productization: settings, backup/import, shortcuts, packaging, and visual polish.

Current runtime boundary: each agent profile can store a shell `launch_command` and optional `working_directory`. If the command is empty, LocalSlock starts a harmless placeholder process so the start/stop/log loop can be tested before wiring a real agent CLI. The desktop app auto-spawns the same binary in `--supervisor` mode; that supervisor owns spawn/kill/log collection through a Postgres command queue. A future launchd wrapper can make the supervisor survive without opening the desktop UI.

The Runtime panel can also install a user LaunchAgent at `~/Library/LaunchAgents/local.localslock.supervisor.plist`. That makes macOS keep the `--supervisor` process alive via `launchctl`; uninstall removes the plist and unloads the service.

## Agent Stdout Event Protocol

An agent process can write structured events back into LocalSlock by printing one line to stdout with the `LOCAL_SLOCK_EVENT ` prefix followed by JSON. Non-matching stdout/stderr is preserved only in the run log.

Examples:

```bash
printf 'LOCAL_SLOCK_EVENT {"type":"message","channel":"local-slock","body":"hello from agent"}\n'
printf 'LOCAL_SLOCK_EVENT {"type":"message","channel":"local-slock","body":"new task","as_task":true}\n'
printf 'LOCAL_SLOCK_EVENT {"type":"task_status","task_number":1,"status":"in_review"}\n'
printf 'LOCAL_SLOCK_EVENT {"type":"task_claim","task_number":1}\n'
```

Supported event types in this slice:

- `message`: requires `channel` or `channel_id`, accepts optional `thread_root_id`, `body`, and `as_task`.
- `task_status`: requires `task_number` and one of `todo`, `in_progress`, `in_review`, `done`.
- `task_claim`: requires `task_number`; defaults to the current agent, or use `assignee_handle` / `unassigned`.

The supervisor injects `LOCAL_SLOCK_AGENT_ID`, `LOCAL_SLOCK_AGENT_HANDLE`, and `LOCAL_SLOCK_RUN_ID` into each agent process.

## Agent Activity Feed

LocalSlock persists agent activity in `agent_activities` instead of deriving it from run logs. The feed is intentionally queryable state: it links activity to an agent and, when available, a run. This gives the UI a stable timeline for:

- profile creation, edits, and deletion;
- queued starts, spawned runs, stop requests, and final run status;
- accepted or rejected stdout events;
- messages and tasks created from stdout events;
- task status and assignee changes made by agents.

Run logs remain useful for debugging one process. The activity feed is the product-level audit trail that future inbox loops, notifications, and filters should build on.

## Agent Launch Presets

The agent form includes editable launch presets for `Codex`, `Claude`, and `Kimi`, plus `Custom`.

Presets are intentionally simple starting points:

- They generate a shell command for the selected CLI and model.
- They embed the LocalSlock stdout event protocol into the command prompt.
- They show a command preview before applying, and the final command remains editable.
- They assume the selected CLI binary is already installed and available on `PATH`.

Use `Custom` when the agent is a local script, a wrapper, or a runtime with its own daemon protocol.

## Run

```bash
npm install
npm run tauri:dev
```

Override the database URL if needed:

```bash
LOCAL_SLOCK_DATABASE_URL=postgres://dylan:123456@127.0.0.1:5432/localslock npm run tauri:dev
```
