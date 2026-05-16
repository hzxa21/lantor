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

The web UI does not perform its own token check. Only expose Lantor on a
trusted private network such as your Tailscale tailnet.

The web UI uses HTTP endpoints for the subset of Tauri commands the chat
surface needs:

- bootstrap
- sending messages
- marking channels read
- completing reminders
- opening agent DMs
- reading artifacts
- workspace preview
- attachment preview

Live refresh is delivered over an SSE stream. Desktop Tauri still uses native
IPC.

## Supervisor LaunchAgent

The Runtime panel can install a user LaunchAgent at:

```text
~/Library/LaunchAgents/local.lantor.supervisor.plist
```

That lets macOS keep the `--supervisor` process alive via `launchctl`.
Uninstall removes the plist and unloads the service.
