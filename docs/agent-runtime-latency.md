# Agent Runtime Latency Notes

## Current LocalSlock Slice

This slice intentionally keeps the runtime protocol unchanged. It reduces visible reply latency by removing local orchestration delays around the existing per-work-item process model.

- UI refresh is now event driven through Postgres `LISTEN/NOTIFY`: backend writers notify `localslock_ui_refresh`, the Tauri process emits `localslock://refresh`, and the React app debounces a bootstrap refresh.
- Supervisor wakeup now uses Postgres `LISTEN/NOTIFY` on `localslock_supervisor_wake` instead of always waiting for the previous 800 ms sleep tick.
- The composer renders owner messages optimistically, then reconciles with the canonical `bootstrap` result after `send_message` returns.

## Slock Daemon Reference

I checked `@slock-ai/daemon@0.43.0`. The daemon does not just run one-shot CLI commands per message. It has runtime-specific drivers and keeps agent process/session state in memory.

Observed structure:

- A `DaemonConnection` holds a WebSocket to the Slock server and receives `agent:start`, `agent:stop`, runtime-profile, and model-detection messages.
- `AgentProcessManager.startAgent` owns per-agent process state: process handle, driver, inbox, session id, idle state, recent stdout/stderr, activity, runtime traces, and gated steering state.
- Claude is launched with `--output-format stream-json --input-format stream-json`, plus MCP config. New turns are sent as JSON lines on stdin.
- Codex is launched as `codex app-server --listen stdio://`. The daemon speaks JSON-RPC over stdin/stdout, using `thread/start`, `thread/resume`, `turn/start`, and `turn/steer`.
- Runtime stdout is parsed into structured events: `thinking`, `text`, `tool_call`, `tool_output`, `compaction_started`, `compaction_finished`, `turn_end`, and `error`.
- Busy delivery is runtime-specific. Codex supports direct same-turn steering through `turn/steer`; Claude uses gated delivery and only injects at safe stream-json boundaries.
- Agents can stay warm after a turn. If a runtime supports stdin notifications and has a session id, the daemon delivers new messages via stdin instead of always cold-spawning a fresh process.

## Recommended Next Protocol Slice

The next big slice should be separate from this latency patch:

1. Introduce a runtime driver abstraction in LocalSlock.
2. Start with Codex because `codex app-server --listen stdio://` gives a clean JSON-RPC protocol for warm sessions and streaming deltas.
3. Store `session_id` / `thread_id` on `agents` or latest `agent_runs`.
4. Convert parsed runtime events into first-class activity rows and optional streaming message drafts.
5. Add Claude after Codex, because Claude needs stream-json boundary gating to avoid unsafe busy stdin injection.

Avoid building warm agents around ad hoc shell stdin for all runtimes. The daemon evidence suggests the stable boundary is per-runtime protocols, not a generic long-lived shell.
