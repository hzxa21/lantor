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
  workItems: AgentWorkItem[];
  onClose: () => void;
  onDelete: (agent: Agent) => void;
  onStart: (agent: Agent) => void;
  onStop: (run: AgentRun) => void;
  onEdit: (agent: Agent) => void;
  onOpenDm: (agent: Agent) => void;
  onOpenWorkItem: (item: AgentWorkItem) => void;
};

const ACTIVITY_PHASE_LABELS: Record<string, string> = {
  thinking: "Thinking",
  acting: "Acting",
  tools: "Using tools",
  error: "Error",
  event: "Acting",
  message: "Acting",
  task: "Acting",
  event_error: "Error",
  run_error: "Error",
};

function phaseForActivity(kind: string) {
  return ACTIVITY_PHASE_LABELS[kind] ?? "Active";
}

function phaseClass(kind: string) {
  return `phase-${kind.replace(/[^a-z0-9_-]/gi, "-")}`;
}

function formatActivityTime(value: string) {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return formatTime(value);
  return date.toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
}

export function AgentDetailDrawer({
  agent,
  activeRun,
  phase,
  activities,
  workItems,
  onClose,
  onDelete,
  onStart,
  onStop,
  onEdit,
  onOpenDm,
  onOpenWorkItem,
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
          <section className="detail-section live-activity-section">
            <div className="detail-section-head">
              <h4>Live activity</h4>
              {activities.length > 0 && <span>Latest {activities.length}</span>}
            </div>
            {activities.length === 0 && <p className="empty-mini">No activity yet.</p>}
            {activities.length > 0 && (
              <div className="activity-timeline" role="log" aria-label={`${agent.handle} activity`}>
                {activities.map((activity) => (
                  <article
                    key={activity.id}
                    className={`activity-timeline-row ${expandedActivityId === activity.id ? "expanded" : ""}`}
                    data-kind={activity.kind}
                    onClick={() => setExpandedActivityId((current) => current === activity.id ? null : activity.id)}
                  >
                    <time>{formatActivityTime(activity.created_at)}</time>
                    <span className="activity-dot" data-kind={activity.kind} aria-hidden="true" />
                    <div className="activity-timeline-body">
                      <div className="activity-timeline-title">
                        <strong>{activity.title}</strong>
                        <span>{phaseForActivity(activity.kind)}</span>
                      </div>
                      <p title="Click to expand activity detail">{activity.detail}</p>
                    </div>
                  </article>
                ))}
              </div>
            )}
          </section>
          <section className="detail-section">
            <h4>Work items</h4>
            {workItems.length === 0 && <p className="empty-mini">No work assigned yet.</p>}
            {workItems.map((item) => (
              <article key={item.id} className="detail-work" onClick={() => onOpenWorkItem(item)}>
                <strong>{item.title}</strong>
                <span>{item.status}{item.task_number ? ` · task #${item.task_number}` : ""}</span>
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
