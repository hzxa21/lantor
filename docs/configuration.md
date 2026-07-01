# Configuration

Lantor reads configuration from environment variables. All of them are
optional — the defaults work for local development. The most common ones
also appear in [`.env.example`](../.env.example).

## Core

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_DATABASE_URL` | `sqlite://~/Library/Application Support/Lantor/lantor.sqlite` | SQLite database URL used by both the desktop process and the web server. `DATABASE_URL` is honored as a fallback only when it is also a `sqlite:` URL. |
| `LANTOR_WEB_BIND` | `0.0.0.0:8787` | Address the embedded web/SSE server binds to. Set to `127.0.0.1:8787` to stay loopback-only, or `off` / `none` to disable the browser UI entirely. |

## Web UI

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_WEB_PUBLIC_URL` | derived from `LANTOR_WEB_BIND` | Public base URL Lantor uses when generating links that point at itself. Set this when the bind address is not directly reachable (for example behind a reverse proxy or when using a Tailscale MagicDNS name). |
| `LANTOR_WEB_DIST` | auto-detected `dist/` next to the repo or current working directory | Override the static web bundle directory served by the desktop process. Useful when running a packaged build from a custom location. |
| `LANTOR_WEB_PIN` | unset | Optional 6-digit PIN required before the browser UI can call Lantor APIs. If set to a non-6-digit value, Lantor disables web access instead of serving it unprotected. |
| `LANTOR_WEB_PIN_MAX_FAILURES` | `10` | Failed PIN attempts allowed before browser login locks. Unlock from the host with the `sqlite3` command shown on the locked login page. |

## Attachments

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_ATTACHMENT_DIR` | `~/Library/Application Support/Lantor/attachments` | Disk location where Lantor copies inbound message attachments. Point this at an alternative volume if you want attachments stored outside `~/Library`. |

## Runtime tuning

| Variable | Default | Purpose |
| --- | --- | --- |
| `LANTOR_CODEX_CONTEXT_ROTATE_INPUT_TOKENS` | `180000` | Rotate warm Codex sessions when the last stopped run exceeds this many input tokens. Lantor ignores values below `50000` to avoid churning sessions unnecessarily. |

## Notes

- Set `LANTOR_WEB_PIN` before exposing the web UI beyond loopback. The PIN gate
  protects browser HTTP APIs only; the desktop app keeps using native Tauri IPC.
  Use HTTPS or a private network when sending the PIN over the network.
- Schema migrations run automatically the first time the desktop process
  connects to a fresh database — there is no separate migration step.
- Agent-local environment such as `LANTOR_AGENT_ID`, `LANTOR_RUN_ID`,
  `LANTOR_CONTEXT_TOOL`, and `LANTOR_WORK_ITEM_*` is injected into agent
  subprocesses by the supervisor and is not meant to be set by users.
