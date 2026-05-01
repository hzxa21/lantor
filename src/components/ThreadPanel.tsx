import { Bot, MessageSquare, Reply, Sparkles, Trash2, X } from "lucide-react";
import { AgentWorkItem, Bootstrap, Channel, Message, TASK_STATUSES, Task } from "../types";
import { firstLines, formatTime } from "../ui-utils";

type ThreadPanelProps = {
  data: Bootstrap;
  channel: Channel | null;
  activeRoot: Message | null;
  activeTask: Task | null;
  replies: Message[];
  workAgentFilter: string;
  workStatusFilter: string;
  visibleWorkItems: AgentWorkItem[];
  queuedWorkItemCount: number;
  taskTitleDrafts: Record<string, string>;
  replyDraft: string;
  setWorkAgentFilter: (value: string) => void;
  setWorkStatusFilter: (value: string) => void;
  openWorkItem: (item: AgentWorkItem) => void;
  cancelWorkItem: (item: AgentWorkItem) => void;
  retryWorkItem: (item: AgentWorkItem) => void;
  installSupervisorService: () => void;
  uninstallSupervisorService: () => void;
  toggleThreadFollow: (message: Message) => void;
  setActiveThreadId: (threadId: string | null) => void;
  setTaskTitleDraft: (task: Task, title: string) => void;
  saveTaskTitle: (task: Task) => void;
  claimTask: (task: Task, agentId: string) => void;
  updateTaskStatus: (task: Task, status: string) => void;
  setReplyDraft: (value: string) => void;
  sendReply: () => void;
};

export function ThreadPanel({
  data,
  channel,
  activeRoot,
  activeTask,
  replies,
  workAgentFilter,
  workStatusFilter,
  visibleWorkItems,
  queuedWorkItemCount,
  taskTitleDrafts,
  replyDraft,
  setWorkAgentFilter,
  setWorkStatusFilter,
  openWorkItem,
  cancelWorkItem,
  retryWorkItem,
  installSupervisorService,
  uninstallSupervisorService,
  toggleThreadFollow,
  setActiveThreadId,
  setTaskTitleDraft,
  saveTaskTitle,
  claimTask,
  updateTaskStatus,
  setReplyDraft,
  sendReply,
}: ThreadPanelProps) {
  return (
    <aside className="thread">
      <header>
        <div>
          <h2>Thread <span>{channel ? `- #${channel.name}` : "- no channel"}</span></h2>
          <p>{activeRoot ? `Root ${activeRoot.id.slice(0, 8)}` : "No thread selected"}</p>
        </div>
        {activeRoot && (
          <button onClick={() => toggleThreadFollow(activeRoot)}>
            {activeRoot.thread_followed ? "Following" : "Muted"}
          </button>
        )}
        <button onClick={() => setActiveThreadId(null)}><X size={18} /></button>
      </header>

      <section className="thread-focus">
        {activeRoot && (
          <article className="thread-root">
            <div className="meta">
              <strong>{activeRoot.sender_name}</strong>
              <time>{formatTime(activeRoot.created_at)}</time>
            </div>
            <p>{activeRoot.body}</p>
          </article>
        )}

        {activeTask && (
          <section className="thread-task-card">
            <div className="task-card-head">
              <span>Task #{activeTask.number}</span>
              <strong>{activeTask.status.replace("_", " ")}</strong>
            </div>
            <input
              value={taskTitleDrafts[activeTask.id] ?? activeTask.title}
              onChange={(event) => setTaskTitleDraft(activeTask, event.target.value)}
              onBlur={() => saveTaskTitle(activeTask)}
              onKeyDown={(event) => {
                if (event.key === "Enter") saveTaskTitle(activeTask);
              }}
            />
            <select
              value={activeTask.assignee_id ?? ""}
              onChange={(event) => claimTask(activeTask, event.target.value)}
            >
              <option value="">Unassigned</option>
              {data.agents.map((agent) => (
                <option key={agent.id} value={agent.id}>{agent.display_name}</option>
              ))}
            </select>
            <div className="status-row">
              {TASK_STATUSES.map((status) => (
                <button
                  key={status}
                  className={activeTask.status === status ? "active" : ""}
                  onClick={() => updateTaskStatus(activeTask, status)}
                >
                  {status.replace("_", " ")}
                </button>
              ))}
            </div>
          </section>
        )}

        <section className="reply-list">
          {!activeRoot && (
            <div className="empty-state compact">
              <MessageSquare size={28} />
              <h2>No thread selected</h2>
              <p>Select a root message after you create one.</p>
            </div>
          )}
          {replies.map((reply) => (
            <article key={reply.id}>
              <div className="avatar tiny">{reply.sender_name.slice(0, 1)}</div>
              <div>
                <div className="meta">
                  <strong>{reply.sender_name}</strong>
                  <time>{formatTime(reply.created_at)}</time>
                </div>
                <p>{reply.body}</p>
              </div>
            </article>
          ))}
        </section>

        <section className="reply-composer">
          <textarea
            value={replyDraft}
            onChange={(event) => setReplyDraft(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter" && !event.shiftKey) {
                event.preventDefault();
                if (activeRoot && replyDraft.trim()) sendReply();
              }
            }}
            disabled={!activeRoot}
            placeholder={activeRoot ? "Reply in thread — Enter to send" : "Select a thread to reply"}
          />
          <button disabled={!activeRoot || !replyDraft.trim()} onClick={sendReply}>
            Reply <Reply size={15} />
          </button>
        </section>
      </section>

      <section className="agent-ops-stack">
        <details className="ops-panel work-panel" open>
          <summary>
            <span>Agent Work</span>
            <strong>{queuedWorkItemCount} queued</strong>
          </summary>
          <p className="ops-hint">Work is created from @mentions in normal messages. This panel only shows status.</p>
          <div className="work-filters">
            <select value={workAgentFilter} onChange={(event) => setWorkAgentFilter(event.target.value)}>
              <option value="">All agents</option>
              {data.agents.map((agent) => (
                <option key={agent.id} value={agent.id}>@{agent.handle}</option>
              ))}
            </select>
            <select value={workStatusFilter} onChange={(event) => setWorkStatusFilter(event.target.value)}>
              <option value="active">Active</option>
              <option value="finished">Finished</option>
              <option value="all">All</option>
              <option value="queued">Queued</option>
              <option value="running">Running</option>
              <option value="done">Done</option>
              <option value="failed">Failed</option>
              <option value="cancelled">Cancelled</option>
            </select>
          </div>
          {data.agent_work_items.length === 0 ? (
            <p className="empty-mini">Dispatched work items will appear here.</p>
          ) : visibleWorkItems.length === 0 ? (
            <p className="empty-mini">No work items match the current filters.</p>
          ) : (
            <div className="work-list">
              {visibleWorkItems.slice(0, 6).map((item) => (
                <article key={item.id} className={`work-card ${item.status}`} onClick={() => openWorkItem(item)}>
                  <div>
                    <strong>{item.title}</strong>
                    <span>
                      @{item.agent_handle} · {item.status}
                      {item.task_number ? ` · task #${item.task_number}` : ""}
                    </span>
                  </div>
                  <div className="work-actions">
                    {["queued", "running", "cancelling"].includes(item.status) && (
                      <button onClick={(event) => {
                        event.stopPropagation();
                        cancelWorkItem(item);
                      }}>
                        Cancel
                      </button>
                    )}
                    {["done", "failed", "cancelled"].includes(item.status) && (
                      <button onClick={(event) => {
                        event.stopPropagation();
                        retryWorkItem(item);
                      }}>
                        Retry
                      </button>
                    )}
                  </div>
                </article>
              ))}
            </div>
          )}
        </details>

        <details className="ops-panel runtime-panel">
          <summary>
            <span>Runtime Runs</span>
            <strong>
              supervisor {data.supervisor.status}
              {data.supervisor.pid ? ` · ${data.supervisor.pid}` : ""}
            </strong>
          </summary>
          <div className="service-card">
            <div>
              <strong>LaunchAgent</strong>
              <span>
                {data.launch_agent.installed ? "installed" : "not installed"} ·{" "}
                {data.launch_agent.loaded ? "loaded" : "not loaded"}
              </span>
              <code>{data.launch_agent.plist_path}</code>
            </div>
            <div className="service-actions">
              <button onClick={installSupervisorService}>
                <Sparkles size={14} /> Install
              </button>
              <button className="danger" onClick={uninstallSupervisorService}>
                <Trash2 size={14} /> Uninstall
              </button>
            </div>
          </div>
          {data.agent_runs.length === 0 && (
            <p className="empty-mini">Start an agent to create the first local run log.</p>
          )}
          {data.agent_runs.slice(0, 5).map((run) => (
            <article key={run.id} className={`run-card ${run.status}`}>
              <div className="run-head">
                <strong>@{run.agent_handle}</strong>
                <span>{run.status}</span>
              </div>
              <code>{run.command}</code>
              <small>
                {formatTime(run.started_at)}
                {run.pid ? ` · pid ${run.pid}` : ""}
                {run.exit_code !== null ? ` · exit ${run.exit_code}` : ""}
              </small>
              {run.log && <pre>{run.log.trim().split("\n").slice(-8).join("\n")}</pre>}
            </article>
          ))}
        </details>

        <details className="ops-panel activity-panel" open>
          <summary>
            <span>Agent Activity</span>
            <strong>{data.agent_activities.length}</strong>
          </summary>
          {data.agent_activities.length === 0 && (
            <p className="empty-mini">Agent activity appears here after profile edits, run lifecycle changes, and stdout events.</p>
          )}
          {data.agent_activities.slice(0, 12).map((activity) => (
            <article key={activity.id} className={`activity-card ${activity.kind}`}>
              <div className="activity-icon">
                {activity.agent_handle.slice(0, 1).toUpperCase() || "A"}
              </div>
              <div>
                <div className="activity-meta">
                  <strong>{activity.title}</strong>
                  <span>{formatTime(activity.created_at)}</span>
                </div>
                <p>{activity.detail || activity.kind}</p>
                <small>@{activity.agent_handle || "unknown"} · {activity.kind}</small>
              </div>
            </article>
          ))}
        </details>

        <section className="db-card">
          <Bot size={18} />
          <div>
            <strong>Postgres State</strong>
            <span>{data.db_url.replace(/:[^:@/]+@/, ":***@")}</span>
          </div>
        </section>
      </section>
    </aside>
  );
}
