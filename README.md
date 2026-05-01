# LocalSlock

Local-only Slock-style desktop console for one human and multiple local agents.

## First Version Scope

- macOS desktop shell with a three-pane layout: channels, chat, thread/task context.
- PostgreSQL state store at `postgres://dylan:123456@127.0.0.1:5432/localslock`.
- Channels, messages, thread replies, tasks, and agent directory state.
- Apple-style visual themes: Liquid Glass, Graphite Pro, Warm Paper.
- No cloud server, no multi-human permissions, no web deployment.

Runtime process control is intentionally left as the next slice. This version establishes the local product shell and data model first.

## Run

```bash
npm install
npm run tauri:dev
```

Override the database URL if needed:

```bash
LOCAL_SLOCK_DATABASE_URL=postgres://dylan:123456@127.0.0.1:5432/localslock npm run tauri:dev
```
