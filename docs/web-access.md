# Tailscale Web Access

Lantor exposes a browser-accessible web UI from the same desktop process so you
can open it from another device, such as a phone over Tailscale. It is enabled
by default on `0.0.0.0:8787`.

```bash
npm run build
npm run tauri:dev
```

Then open the Mac's Tailscale address from the other device:

```text
http://<mac-tailscale-ip>:8787/
```

Loopback requests from the same Mac can use:

```text
http://127.0.0.1:8787/
```

To restrict to loopback, set `LANTOR_WEB_BIND=127.0.0.1:8787`. To turn the web
server off, set `LANTOR_WEB_BIND=off` (also accepts `none`, `disabled`,
`false`, or `0`).

For browser access outside the local machine, set a 6-digit PIN:

```bash
LANTOR_WEB_PIN=123456 npm run tauri:dev
```

The PIN protects all browser API routes except `/api/health` and
`/api/auth/*`. Desktop Tauri commands still use native IPC and do not require
the browser PIN. The default lockout threshold is 10 failed attempts; override
it with `LANTOR_WEB_PIN_MAX_FAILURES`.

After lockout, login stays blocked until someone with shell access to the host
runs the `sqlite3` unlock command shown on the locked login page. It has this
shape:

```bash
sqlite3 '~/Library/Application Support/Lantor/lantor.sqlite' "update web_auth_state set failed_attempts=0, locked_at=null where id='web_pin';"
```

The PIN is sent to the Lantor host during login, so use HTTPS or a trusted
private network such as your Tailscale tailnet.

The web UI uses HTTP endpoints under `/api/` for the subset of Tauri commands
the chat surface needs, including:

- bootstrap and runtime health checks
- sending messages, creating/updating/deleting channels and agents
- managing channel agent membership and saved messages
- inbox dismissal and read state, channel read state
- reminders (completing) and tasks (status, title, claim)
- cancelling and retrying agent work
- installing and uninstalling the supervisor LaunchAgent
- opening agent DMs
- reading artifacts and attachment preview
- agent workspace listing and file preview
- owner profile updates

Live refresh is delivered over an SSE stream at `/api/events`. Desktop Tauri
still uses native IPC for the same operations.

## Supervisor LaunchAgent

The Runtime panel can install a user LaunchAgent at:

```text
~/Library/LaunchAgents/local.lantor.supervisor.plist
```

That lets macOS keep the `--supervisor` process alive via `launchctl`.
Uninstall removes the plist and unloads the service.
