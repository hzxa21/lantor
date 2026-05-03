import { useState } from "react";
import { Agent, AgentActivity, AgentRun, AgentWorkItem } from "../types";
import { formatTime } from "../ui-utils";

type AgentPhase = {
  kind: string;
  label: string;
  detail: string;
};

type AgentDetailDrawerProps = {
  agent: Agent;
  activeRun: AgentRun | null;
  phase: AgentPhase | null;
  activities: AgentActivity[];
  performance: AgentPerformance;
  workItems: AgentWorkItem[];
  onClose: () => void;
  onDelete: (agent: Agent) => void;
  onStart: (agent: Agent) => void;
  onStop: (run: AgentRun) => void;
  onEdit: (agent: Agent) => void;
  onOpenDm: (agent: Agent) => void;
  onOpenWorkItem: (item: AgentWorkItem) => void;
  onCancelWorkItem: (item: AgentWorkItem) => void;
  onRetryWorkItem: (item: AgentWorkItem) => void;
};

export type AgentPerformance = {
  windowLabel: string;
  turns: number;
  completedTurns: number;
  failedTurns: number;
  activeTurns: number;
  p50FirstTokenMs: number | null;
  p95FirstTokenMs: number | null;
  p50TurnMs: number | null;
  p95TurnMs: number | null;
  errorRate: number;
};

const ACTIVITY_STATUS_LABELS: Record<string, string> = {
  active: "Active",
  success: "Done",
  warning: "Needs attention",
  error: "Error",
  info: "Info",
};

function statusForActivity(activity: AgentActivity) {
  return ACTIVITY_STATUS_LABELS[activity.status] ?? activity.status;
}

function userFacingActivityTitle(activity: AgentActivity) {
  const title = activity.summary || activity.title;
  const lowered = title.toLowerCase();
  if (lowered.includes("warm turn") && lowered.includes("started")) return "Started working";
  if (lowered.includes("running command")) return "Running command";
  if (lowered.includes("command finished")) return "Command finished";
  if (lowered.includes("editing file")) return "Editing file";
  if (lowered.includes("file edit finished")) return "File edit finished";
  if (lowered.includes("turn accepted")) return "Request acknowledged";
  if (lowered.includes("turn interrupt") || lowered.includes("stop signal")) return "Stop requested";
  if (lowered.includes("first token")) return "Responding";
  if (lowered.includes("content block finished")) return "Finished step";
  if (lowered.includes("message finished")) return "Finished response";
  if (lowered.includes("stream initialized")) return "Runtime ready";
  if (lowered.includes("system event") || lowered.includes("status")) return "Runtime status";
  if (lowered.includes("rate limit")) return "Checking rate limit";
  if (lowered.includes("follow-up steer")) return title.includes("rejected") ? "Follow-up queued" : "Follow-up added";
  if (lowered.startsWith("work item ")) {
    if (lowered.includes("running")) return "Request started";
    if (lowered.includes("done")) return "Request completed";
    if (lowered.includes("failed")) return "Request failed";
    if (lowered.includes("cancelled")) return "Request cancelled";
  }
  return title;
}

function userFacingActivityCategory(activity: AgentActivity) {
  if (activity.status === "error") return "Error";
  switch (activity.phase || activity.kind) {
    case "thinking":
      return "Thinking";
    case "command":
      return "Command";
    case "file_edit":
      return "File edit";
    case "tools":
      return "Tool";
    case "acting":
      return "Response";
    case "work":
      return "Request";
    case "runtime":
      return "Runtime";
    case "profile":
      return "Profile";
    default:
      return "Working";
  }
}

function userFacingActivityDetail(activity: AgentActivity) {
  const detail = activity.detail.trim();
  if (!detail) return "";
  if (/^[0-9a-f]{8}-[0-9a-f-]{27,}$/i.test(detail)) return "";
  if (["content_block_stop", "message_stop", "status"].includes(detail)) return "";
  return detail
    .replace(/^codex warm turn failed:/i, "Failed:")
    .replace(/^claude warm turn failed:/i, "Failed:")
    .replace(/^duration=/i, "duration ");
}

function phaseClass(kind: string) {
  return `phase-${kind.replace(/[^a-z0-9_-]/gi, "-")}`;
}

function stringifyMetadata(value: unknown) {
  if (value === null || value === undefined) return "";
  if (typeof value === "string" || typeof value === "number" || typeof value === "boolean") {
    return String(value);
  }
  return JSON.stringify(value);
}

function compactValue(value: string, maxLength = 54) {
  const normalized = value.replace(/\s+/g, " ").trim();
  if (normalized.length <= maxLength) return normalized;
  return `${normalized.slice(0, maxLength - 1)}…`;
}

function metadataEntries(activity: AgentActivity) {
  return Object.entries(activity.metadata ?? {})
    .filter(([key]) => !["detail", "reference_id", "run_id", "thread_id"].includes(key))
    .map(([key, value]) => [key, stringifyMetadata(value)] as const)
    .filter(([, value]) => value.length > 0);
}

function visibleMetadataEntries(activity: AgentActivity) {
  const priority = ["command", "file", "operation", "tool", "duration_ms", "exit_code", "status", "type", "reason"];
  const entries = metadataEntries(activity)
    .filter(([key]) => !["rate_limit_info", "uuid", "pid", "session_id", "request_id"].includes(key));

  return entries
    .sort(([left], [right]) => {
      const leftIndex = priority.indexOf(left);
      const rightIndex = priority.indexOf(right);
      if (leftIndex === -1 && rightIndex === -1) return 0;
      if (leftIndex === -1) return 1;
      if (rightIndex === -1) return -1;
      return leftIndex - rightIndex;
    })
    .slice(0, 3);
}

function formatActivityTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return formatTime(value);
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

function formatDuration(value: number | null) {
  if (value === null || Number.isNaN(value)) return "n/a";
  if (value < 1000) return `${Math.round(value)} ms`;
  return `${(value / 1000).toFixed(value < 10_000 ? 1 : 0)} s`;
}

export function AgentDetailDrawer({
  agent,
  activeRun,
  phase,
  activities,
  performance,
  workItems,
  onClose,
  onDelete,
  onStart,
  onStop,
  onEdit,
  onOpenDm,
  onOpenWorkItem,
  onCancelWorkItem,
  onRetryWorkItem,
}: AgentDetailDrawerProps) {
  const [expandedActivityId, setExpandedActivityId] = useState<string | null>(null);

  return (
    <aside className="agent-drawer">
      <header className="agent-drawer-head">
        <div>
          <span>Agent</span>
          <h2>@{agent.handle}</h2>
        </div>
        <button onClick={onClose} aria-label="Close agent detail">×</button>
      </header>
      <div className="agent-drawer-body">
        <div className="agent-detail">
          <section className="agent-detail-hero">
            <div className="avatar large">{agent.avatar || agent.handle.slice(0, 1).toUpperCase()}</div>
            <div>
              <h3>{agent.display_name}</h3>
              <p>@{agent.handle} · {agent.runtime} · {agent.model}</p>
              {phase && (
                <div className="agent-phase-line">
                  <span className={`phase-badge ${phaseClass(phase.kind)}`}>{phase.label}</span>
                  <small>{phase.detail}</small>
                </div>
              )}
            </div>
            <span className={`status-badge ${agent.status}`}>{agent.status}</span>
          </section>
          <section className="detail-grid">
            <div>
              <span>Workspace</span>
              <code>{agent.working_directory || "Not configured"}</code>
            </div>
            <div>
              <span>Active run</span>
              <code>{activeRun ? `${activeRun.status}${activeRun.pid ? ` · pid ${activeRun.pid}` : ""}` : "No active run"}</code>
            </div>
            <div>
              <span>Role</span>
              <code>{agent.role || "agent"}</code>
            </div>
            <div>
              <span>Description</span>
              <code>{agent.description || "No notes"}</code>
            </div>
          </section>
          <section className="detail-section performance-section">
            <div className="detail-section-head">
              <h4>Performance</h4>
              <span>{performance.windowLabel}</span>
            </div>
            <div className="performance-grid">
              <div className="performance-card">
                <span>Turns</span>
                <strong>{performance.turns}</strong>
                <small>{performance.completedTurns} done · {performance.failedTurns} failed</small>
              </div>
              <div className="performance-card">
                <span>First token</span>
                <strong>{formatDuration(performance.p50FirstTokenMs)}</strong>
                <small>p95 {formatDuration(performance.p95FirstTokenMs)}</small>
              </div>
              <div className="performance-card">
                <span>Turn duration</span>
                <strong>{formatDuration(performance.p50TurnMs)}</strong>
                <small>p95 {formatDuration(performance.p95TurnMs)}</small>
              </div>
              <div className="performance-card">
                <span>Error rate</span>
                <strong>{Math.round(performance.errorRate * 100)}%</strong>
                <small>{performance.activeTurns} currently active</small>
              </div>
            </div>
          </section>
          <section className="detail-section live-activity-section">
            <div className="detail-section-head">
              <h4>Live activity</h4>
              {activities.length > 0 && <span>Latest {activities.length}</span>}
            </div>
            {activities.length === 0 && <p className="empty-mini">No activity yet.</p>}
            {activities.length > 0 && (
              <div className="activity-timeline" role="log" aria-label={`${agent.handle} activity`}>
                {activities.map((activity) => {
                  const title = userFacingActivityTitle(activity);
                  const detail = userFacingActivityDetail(activity);
                  const category = userFacingActivityCategory(activity);
                  const visibleMetadata = visibleMetadataEntries(activity);
                  const allMetadata = metadataEntries(activity);
                  return (
                    <article
                      key={activity.id}
                      className={`activity-timeline-row ${expandedActivityId === activity.id ? "expanded" : ""}`}
                      data-kind={activity.kind}
                      data-phase={activity.phase}
                      data-status={activity.status}
                      onClick={() => setExpandedActivityId((current) => current === activity.id ? null : activity.id)}
                    >
                      <time>{formatActivityTime(activity.created_at)}</time>
                      <span
                        className="activity-dot"
                        data-kind={activity.kind}
                        data-phase={activity.phase}
                        data-status={activity.status}
                        aria-hidden="true"
                      />
                      <div className="activity-timeline-body">
                        <div className="activity-timeline-title">
                          <strong>{title}</strong>
                          <span className={`activity-status status-${activity.status}`}>
                            {statusForActivity(activity)}
                          </span>
                        </div>
                        <div className="activity-structure-line">
                          <span>{category}</span>
                          {activity.status === "active" && <span>In progress</span>}
                        </div>
                        {visibleMetadata.length > 0 && (
                          <div className="activity-metadata">
                            {visibleMetadata.map(([key, value]) => (
                              <span key={key} title={`${key}: ${value}`}>
                                <b>{key}</b>
                                {compactValue(value)}
                              </span>
                            ))}
                          </div>
                        )}
                        {detail && (
                          <p title="Click to expand activity detail">{compactValue(detail, 120)}</p>
                        )}
                        {expandedActivityId === activity.id && allMetadata.length > 0 && (
                          <pre className="activity-raw">{JSON.stringify(activity.metadata, null, 2)}</pre>
                        )}
                      </div>
                    </article>
                  );
                })}
              </div>
            )}
          </section>
          <section className="detail-section">
            <h4>Work items</h4>
            {workItems.length === 0 && <p className="empty-mini">No work assigned yet.</p>}
            {workItems.map((item) => (
              <article key={item.id} className="detail-work" onClick={() => onOpenWorkItem(item)}>
                <div>
                  <strong>{item.title}</strong>
                  <span>{item.status}{item.task_number ? ` · task #${item.task_number}` : ""}</span>
                </div>
                <div className="detail-work-actions">
                  {["queued", "running", "cancelling"].includes(item.status) && (
                    <button
                      className="danger"
                      disabled={item.status === "cancelling"}
                      onClick={(event) => {
                        event.stopPropagation();
                        onCancelWorkItem(item);
                      }}
                    >
                      {item.status === "cancelling" ? "Cancelling" : "Cancel"}
                    </button>
                  )}
                  {["failed", "cancelled"].includes(item.status) && (
                    <button
                      onClick={(event) => {
                        event.stopPropagation();
                        onRetryWorkItem(item);
                      }}
                    >
                      Retry
                    </button>
                  )}
                </div>
              </article>
            ))}
          </section>
        </div>
      </div>
      <footer className="agent-drawer-actions">
        <button className="danger" onClick={() => onDelete(agent)}>Delete</button>
        <div>
          <button onClick={() => onOpenDm(agent)}>Open DM</button>
          {activeRun ? (
            <button onClick={() => onStop(activeRun)}>Stop</button>
          ) : (
            <button onClick={() => onStart(agent)}>Start</button>
          )}
          <button className="primary" onClick={() => onEdit(agent)}>Edit</button>
        </div>
      </footer>
    </aside>
  );
}
