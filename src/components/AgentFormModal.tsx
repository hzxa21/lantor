import { Modal } from "./Modal";
import { AgentForm, RuntimeCheck, modelOptionsForRuntime } from "../types";
import { APP_DISPLAY_NAME } from "../branding";

type AgentFormModalProps = {
  open: boolean;
  title: string;
  form: AgentForm;
  runtimeChecks: Record<string, RuntimeCheck>;
  submitLabel: string;
  showNotes?: boolean;
  onChange: (form: AgentForm) => void;
  onRuntimeChange: (runtime: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
};

function RuntimePreflight({ check }: { check: RuntimeCheck | undefined }) {
  if (!check) {
    return <div className="runtime-preflight pending">Checking local CLI...</div>;
  }
  return (
    <div className={`runtime-preflight ${check.available ? "ok" : "missing"}`}>
      <strong>{check.available ? "Runtime ready" : "Runtime unavailable"}</strong>
      <span>{check.command || check.runtime}: {check.detail}</span>
    </div>
  );
}

export function AgentFormModal({
  open,
  title,
  form,
  runtimeChecks,
  submitLabel,
  showNotes = false,
  onChange,
  onRuntimeChange,
  onCancel,
  onSubmit,
}: AgentFormModalProps) {
  return (
    <Modal open={open} title={title} onClose={onCancel} width={700}>
      <div className="modal-form agent-modal-form">
        <div className="two-col">
          <label>
            <span>Handle</span>
            <input
              autoFocus
              value={form.handle}
              onChange={(event) => onChange({ ...form, handle: event.target.value })}
              placeholder="@agent"
            />
          </label>
          <label>
            <span>Display name</span>
            <input
              value={form.displayName}
              onChange={(event) => onChange({ ...form, displayName: event.target.value })}
              placeholder="display name"
            />
          </label>
        </div>
        <div className="two-col">
          <label>
            <span>Runtime</span>
            <select value={form.runtime} onChange={(event) => onRuntimeChange(event.target.value)}>
              <option value="codex">Codex</option>
              <option value="claude">Claude</option>
              <option value="kimi">Kimi</option>
            </select>
          </label>
          <label>
            <span>Model</span>
            <select
              value={form.model}
              onChange={(event) => onChange({ ...form, model: event.target.value })}
            >
              {modelOptionsForRuntime(form.runtime, form.model).map((model) => (
                <option key={model} value={model}>{model}</option>
              ))}
            </select>
          </label>
        </div>
        <div className="two-col">
          <label>
            <span>Role</span>
            <input
              value={form.role}
              onChange={(event) => onChange({ ...form, role: event.target.value })}
              placeholder="reviewer, builder, analyst"
            />
          </label>
          <label>
            <span>Avatar</span>
            <input
              value={form.avatar}
              onChange={(event) => onChange({ ...form, avatar: event.target.value })}
              placeholder="emoji, initials, URL, or dicebear:bottts-neutral"
            />
          </label>
        </div>
        <label>
          <span>Daily budget</span>
          <input
            value={form.dailyBudgetUsd}
            inputMode="decimal"
            onChange={(event) => onChange({ ...form, dailyBudgetUsd: event.target.value })}
            placeholder="USD per day, blank = unlimited"
          />
        </label>
        <RuntimePreflight check={runtimeChecks[form.runtime]} />
        {showNotes && (
          <label>
            <span>Notes</span>
            <textarea
              value={form.description}
              onChange={(event) => onChange({ ...form, description: event.target.value })}
              placeholder="Agent notes"
            />
          </label>
        )}
        <label>
          <span>Workspace directory</span>
          <input
            value={form.workingDirectory}
            onChange={(event) => onChange({ ...form, workingDirectory: event.target.value })}
            placeholder="~/Library/Application Support/Lantor/agents/<handle>"
          />
          <small>{APP_DISPLAY_NAME} loads MEMORY.md from this directory as persistent context when the agent runs.</small>
        </label>
        <div className="modal-actions">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!form.handle.trim()} onClick={onSubmit}>{submitLabel}</button>
        </div>
      </div>
    </Modal>
  );
}
