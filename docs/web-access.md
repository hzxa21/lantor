# Tailscale Web Access

Lantor exposes a browser-accessible web UI from the same desktop process so you
can open it from another device, such as a phone over Tailscale. It is enabled
by default on `0.0.0.0:8787`.

```bash
npm run build
npm run tauri:dev
```

Then open the host machine's Tailscale address from the other device:

```text
http://<mac-tailscale-ip>:8787/
```

Loopback requests from the same machine can use:

```text
http://127.0.0.1:8787/
```

To restrict to loopback, set `LANTOR_WEB_BIND=127.0.0.1:8787`. To turn the web
server off, set `LANTOR_WEB_BIND=off` (also accepts `none`, `disabled`,
`false`, or `0`).

The web UI does not perform its own token check. Only expose Lantor on a
trusted private network such as your Tailscale tailnet.

The web UI uses HTTP endpoints under `/api/` for the subset of Tauri commands
the chat surface needs, including:

- bootstrap and runtime health checks
- sending messages, creating/updating/deleting channels and agents
- managing channel agent membership and saved messages
- inbox dismissal and read state, channel read state
- reminders (completing) and tasks (status, title, claim)
- cancelling and retrying agent work
- installing and uninstalling the supervisor background service
- opening agent DMs
- reading artifacts and attachment preview
- agent workspace listing and file preview
- owner profile updates

Live refresh is delivered over an SSE stream at `/api/events`. Desktop Tauri
still uses native IPC for the same operations.

## Supervisor Service

The Runtime panel can install a user background service. On macOS this is a
LaunchAgent at:

```text
~/Library/LaunchAgents/local.lantor.supervisor.plist
```

On Linux this is a systemd user unit at:

```text
~/.config/systemd/user/local.lantor.supervisor.service
```

That lets macOS keep the `--supervisor` process alive via `launchctl`, and
Linux keep it alive via `systemctl --user`. Uninstall removes the service file
and unloads the service.
