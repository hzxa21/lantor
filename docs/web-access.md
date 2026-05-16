# Tailscale Web Access

Lantor can expose a browser-accessible web UI from the same desktop process for
private Tailscale access from another device, such as a phone. It is disabled
by default.

```bash
npm run build
LANTOR_WEB_BIND=0.0.0.0:8787 npm run tauri:dev
```

Then open the Mac's Tailscale address from the other device:

```text
http://<mac-tailscale-ip>:8787/
```

Loopback requests from the same Mac can use:

```text
http://127.0.0.1:8787/
```

The web UI does not perform its own token check. Keep the bind on `127.0.0.1`
unless you are intentionally exposing Lantor on a trusted private network such
as your Tailscale tailnet.

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
