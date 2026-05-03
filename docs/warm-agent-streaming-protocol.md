# Warm Agent and Streaming Runtime Protocol

## Goal

LocalSlock currently starts one CLI process per work item and waits for `LOCAL_SLOCK_EVENT` lines to create visible messages. That is simple, but it pays CLI cold-start cost for every reply and cannot show token-level progress.

The target model is:

- Keep a runtime process warm per active agent when the runtime supports it.
- Deliver new work to that process over the runtime's native stdin protocol.
- Parse runtime-native stdout into structured activity and streaming message updates.
- Keep the existing one-shot `LOCAL_SLOCK_EVENT` path as a compatibility fallback.

Current implementation status:

- Codex uses `codex app-server --listen stdio://` for real-time JSON output parsing.
- LocalSlock streams `item/agentMessage/delta` into visible `messages` rows with `delivery_state = 'streaming'`.
- Codex thread ids are persisted in `runtime_sessions` and resumed on the next work item.
- The process is still per-work-item in this slice. The next warm-runtime slice should keep the app-server process resident and deliver queued work through `turn/start` / `turn/steer`.

## Non-Goals

- Do not build a generic long-lived `/bin/zsh` shell protocol.
- Do not make all runtimes share one stdin format.
- Do not implement Claude busy-turn injection before Codex warm sessions are stable.
- Do not replace the current message/task schema in the first implementation slice.

## Slock Daemon Reference

`@slock-ai/daemon@0.43.0` uses runtime-specific drivers:

- Codex: `codex app-server --listen stdio://`, JSON request/notification protocol over stdin/stdout. Framing is one JSON object per line; requests use `method` / `id` / `params` and do not include a `jsonrpc` field.
- Claude: `claude --output-format stream-json --input-format stream-json`, JSON lines over stdin/stdout.
- OpenCode: per-turn process in the current daemon version, not the first LocalSlock target.

The daemon keeps per-agent process state in memory: process handle, runtime driver, inbox, session id, idle flag, recent stdout/stderr, activity state, runtime trace state, and delivery gating state.

The key design lesson is that "warm agent" is a runtime-driver problem, not a shell-management problem.

## Data Model

Add a runtime session table. Keep it separate from `agents` so session lifecycle can be reset without rewriting agent profile data.

```sql
create table if not exists runtime_sessions (
    id uuid primary key default gen_random_uuid(),
    agent_id uuid not null references agents(id) on delete cascade,
    runtime text not null,
    provider_thread_id text not null,
    status text not null default 'idle',
    created_at timestamptz not null default now(),
    updated_at timestamptz not null default now(),
    unique(agent_id, runtime)
);
```

Extend messages for streaming state:

```sql
alter table messages
  add column if not exists delivery_state text not null default 'complete',
  add column if not exists stream_key text not null default '';
```

Initial values:

- `delivery_state = 'streaming'`: visible draft being appended.
- `delivery_state = 'complete'`: final message.
- `delivery_state = 'error'`: runtime failed while producing this draft.

The first implementation can update `messages.body` in place. A separate `message_chunks` table is not needed until replay/debugging requires token-level history.

## Backend Architecture

Introduce a runtime module boundary:

```rust
enum RuntimeEvent {
    SessionInit { session_id: String },
    Thinking { text: String },
    TextDelta { text: String },
    ToolCall { name: String, input: serde_json::Value },
    ToolOutput { name: String },
    TurnEnd,
    Error { message: String },
}

trait RuntimeDriver {
    fn runtime_id(&self) -> &'static str;
    async fn spawn(&self, ctx: RuntimeSpawnContext) -> CommandResult<RuntimeProcess>;
    async fn send_turn(&self, process: &mut RuntimeProcess, prompt: String) -> CommandResult<()>;
    async fn steer_turn(&self, process: &mut RuntimeProcess, prompt: String) -> CommandResult<()>;
    fn parse_stdout_line(&mut self, line: &str) -> Vec<RuntimeEvent>;
}
```

Rust cannot use async trait methods without extra boxing or `async_trait`. For minimal dependency churn, implement this as an enum dispatcher first:

```rust
enum RuntimeDriverKind {
    OneShot,
    CodexWarm(CodexWarmDriver),
    ClaudeStream(ClaudeStreamDriver),
}
```

## Codex Warm Driver

Codex should be first because its app-server protocol has explicit JSON request ids and structured streaming notifications.

### Spawn

```text
codex app-server --listen stdio://
```

Pass the same safety config currently implied by LocalSlock:

- `cwd`: agent working directory.
- `approvalPolicy`: `never`.
- `sandbox`: `danger-full-access`.
- model from agent config.
- developer instructions from the LocalSlock standing prompt.

### Startup State Machine

1. Spawn process with stdin/stdout pipes.
2. Send:

```json
{"id":1,"method":"initialize","params":{"clientInfo":{"name":"localslock","title":"LocalSlock","version":"0.1.0"},"capabilities":{"experimentalApi":true}}}
```

3. On initialize result, send `initialized` notification.
4. If `runtime_sessions.provider_thread_id` exists, send `thread/resume`; otherwise send `thread/start`.
5. Store returned thread id as `runtime_sessions.provider_thread_id`.
6. For a new work item, send `turn/start`.
7. If a work item arrives while a turn is active, use `turn/steer` only for Codex.

### Input Methods

Start a new turn:

```json
{
  "id": 2,
  "method": "turn/start",
  "params": {
    "threadId": "<session_id>",
    "input": [{ "type": "text", "text": "<work item prompt>", "text_elements": [] }]
  }
}
```

Steer an active turn:

```json
{
  "id": 3,
  "method": "turn/steer",
  "params": {
    "threadId": "<session_id>",
    "expectedTurnId": "<active_turn_id>",
    "input": [{ "type": "text", "text": "<new user message>", "text_elements": [] }]
  }
}
```

### Output Mapping

Map stdout JSON notifications to LocalSlock events:

- `thread/started` or thread result -> `SessionInit`.
- `turn/started` -> mark `agent_work_items.status = 'running'`.
- `item/agentMessage/delta` -> `TextDelta`.
- `item/reasoning/summaryTextDelta` / `item/reasoning/textDelta` -> `Thinking`.
- `item/started` with `commandExecution`, `mcpToolCall`, `webSearch`, `fileChange` -> `ToolCall`.
- `item/completed` with tool-like items -> `ToolOutput`.
- `turn/completed` -> `TurnEnd`.
- Error response or `error` notification -> `Error`.

## Streaming Message Contract

For the first streaming slice, the runtime manager owns draft messages.

1. On first `TextDelta`, insert an agent message:

```sql
insert into messages (
  channel_id,
  thread_root_id,
  sender_agent_id,
  sender_name,
  sender_role,
  body,
  is_task,
  delivery_state,
  stream_key
) values (..., '', false, 'streaming', '<run_id>:assistant');
```

2. On each `TextDelta`, append to `messages.body`.
3. On `TurnEnd`, set `delivery_state = 'complete'`.
4. On `Error`, set `delivery_state = 'error'` and append a short error note if no body exists.
5. Emit the existing UI refresh notification after each append, initially debounced on the frontend.

This keeps the UI simple and avoids adding chunk history until needed.

## Activity Contract

Runtime events should populate `agent_activities`:

- `Thinking` -> `kind = 'thinking'`, title `Thinking`.
- `ToolCall` -> `kind = 'tools'`, title `Using <tool>`.
- `ToolOutput` -> `kind = 'tools'`, title `<tool> finished`.
- `TextDelta` -> do not create one activity per token; only update the streaming message.
- `TurnEnd` -> `kind = 'run'`, title `Turn completed`.
- `Error` -> `kind = 'error'`, title `Runtime error`.

Throttle repeated thinking updates. A practical first rule is one activity row per 500 ms or per phase transition.

## Supervisor Scheduling Changes

Current behavior:

- Every work item inserts a `start_agent` supervisor command.
- Supervisor starts a fresh process for that command.

Warm behavior:

1. If the target runtime supports warm sessions and no process is running, start the runtime process.
2. If the process is idle, deliver the work item via `turn/start`.
3. If the process is busy and supports safe steering, deliver via `turn/steer`.
4. If the process is busy and does not support steering, keep the work item queued.
5. On `TurnEnd`, immediately schedule the next queued item for that agent.

The one-shot driver keeps the current `start_agent` command semantics.

## Failure Recovery

Required behavior:

- Process exits while `delivery_state = 'streaming'`: mark message `error`, work item `failed`, session `status = 'failed'`.
- Invalid JSON line: append run log, emit `error` activity after repeated failures, keep process alive.
- Lost session id on disk/runtime side: clear `runtime_sessions.provider_thread_id` and cold-start a new thread.
- `turn/steer` rejected because turn id changed: requeue the message and retry with `turn/start` after current turn ends.
- User stops agent: terminate process group, mark session stopped, do not delete `provider_thread_id` unless user explicitly resets context.

## Implementation Plan

### Slice 1: Runtime Session Schema and One-Shot Driver Adapter

- Add `runtime_sessions` and `messages.delivery_state`.
- Wrap current CLI launch path as `OneShotDriver`.
- No behavior change expected.
- Verify current agent reply, DM reply, stop, retry, and activity flows still pass.

### Slice 2: Codex Warm Driver

- Implement `CodexWarmDriver`.
- Persist and resume `provider_thread_id`.
- Deliver queued work via `turn/start`.
- Keep `LOCAL_SLOCK_EVENT` handling available for final explicit messages.
- Verify cold first turn, warm second turn, stop, process crash recovery.

### Slice 3: Streaming Draft Messages

- Convert Codex `TextDelta` into draft agent messages.
- Add UI styling for `delivery_state`.
- Keep final `LOCAL_SLOCK_EVENT message` compatibility: if an explicit message event is emitted, finalize or replace the draft instead of duplicating replies.

### Slice 4: Busy Steering for Codex

- Add `turn/steer` for user messages arriving during an active turn.
- If rejected, requeue behind current turn.
- Show inline activity: "Message delivered to running turn" vs "Queued for next turn".

### Slice 5: Claude Stream Driver

- Launch Claude with stream-json input/output.
- Parse thinking/text/tool/result events.
- Implement gated delivery, not direct busy stdin.
- Reuse streaming draft message path.

## Open Questions

- Whether explicit `LOCAL_SLOCK_EVENT message` should replace the streaming draft or appear as a second final message. Recommended: replace/finalize the draft when it belongs to the same run.
- Whether warm sessions should be always-on or only retained for N idle minutes. Recommended first default: keep warm until user stops agent or app exits.
- Whether DM messages should always steer active turns. Recommended: yes for Codex, gated for Claude, queued for one-shot.
- Whether stream updates should push full bootstrap or patch messages. Recommended first: full bootstrap with debounce; later: event payload patches.
