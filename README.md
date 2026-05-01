# LocalSlock

Local-only Slock-style desktop console for one human and multiple local agents.

## First Version Scope

- macOS desktop shell with a three-pane layout: channels, chat, thread/task context.
- PostgreSQL state store at `postgres://dylan:123456@127.0.0.1:5432/localslock`.
- Clean initialization: the app creates schema only and does not seed demo data.
- UI operations for channels, messages, thread replies, tasks, and agent profiles.
- Single Apple-style Liquid Glass visual direction.
- No cloud server, no multi-human permissions, no web deployment.

Runtime process control is intentionally left as the next slice. This version establishes the local product shell and data model first.

## Iteration Path

1. MVP operations: create/edit/delete channels and agents, root messages, thread replies, task claim/status.
2. Agent runtime: launch/stop local Codex, Claude, and Kimi processes with logs and status.
3. Collaboration semantics: channel membership, thread follow/unfollow, search, and local notifications.
4. Desktop productization: settings, backup/import, shortcuts, packaging, and visual polish.

## Run

```bash
npm install
npm run tauri:dev
```

Override the database URL if needed:

```bash
LOCAL_SLOCK_DATABASE_URL=postgres://dylan:123456@127.0.0.1:5432/localslock npm run tauri:dev
```
