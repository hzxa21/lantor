import { Modal } from "./Modal";
import {
  AgentForm,
  CODEX_REASONING_EFFORTS,
  CODEX_SERVICE_TIERS,
  RuntimeCheck,
  modelLabel,
  modelOptionsForRuntime,
} from "../types";
import { APP_DISPLAY_NAME } from "../branding";
import { AgentAvatar } from "./AgentAvatar";
import { AvatarInput } from "./AvatarInput";

type AgentFormModalProps = {
  open: boolean;
  title: string;
  form: AgentForm;
  runtimeChecks: Record<string, RuntimeCheck>;
  submitLabel: string;
  createMode?: boolean;
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

function seedForAgentForm(form: AgentForm) {
  const seed = [form.handle, form.displayName, form.role]
    .map((value) => value.trim())
    .filter(Boolean)
    .join(":");
  return seed || "agent";
}

export function AgentFormModal({
  open,
  title,
  form,
  runtimeChecks,
  submitLabel,
  createMode = false,
  showNotes = false,
  onChange,
  onRuntimeChange,
  onCancel,
  onSubmit,
}: AgentFormModalProps) {
  const isCodex = form.runtime === "codex";
  const previewHandle = (form.handle || form.displayName || "agent").trim().replace(/^@/, "") || "agent";
  const previewName = form.displayName.trim() || form.handle.trim().replace(/^@/, "") || "New agent";
  const previewAgent = {
    id: "agent-form-preview",
    handle: previewHandle,
    display_name: previewName,
    status: "idle",
    runtime: form.runtime,
    model: form.model,
    role: form.role,
    avatar: form.avatar,
  };
  const canSubmit = createMode
    ? Boolean(form.displayName.trim() || form.handle.trim())
    : Boolean(form.handle.trim() || form.displayName.trim());
  const runtimeSelect = (
    <label>
      <span>Runtime</span>
      <select value={form.runtime} onChange={(event) => onRuntimeChange(event.target.value)}>
        <option value="codex">Codex</option>
        <option value="claude">Claude</option>
      </select>
    </label>
  );
  const modelSelect = (
    <label>
      <span>Model</span>
      <select
        value={form.model}
        onChange={(event) => onChange({ ...form, model: event.target.value })}
      >
        {modelOptionsForRuntime(form.runtime, form.model).map((model) => (
          <option key={model} value={model}>{modelLabel(model)}</option>
        ))}
      </select>
    </label>
  );
  const codexControls = isCodex && (
    <>
      <label>
        <span>Intelligence</span>
        <select
          value={form.reasoningEffort}
          onChange={(event) => onChange({ ...form, reasoningEffort: event.target.value })}
        >
          {CODEX_REASONING_EFFORTS.map((effort) => (
            <option key={effort.value} value={effort.value}>{effort.label}</option>
          ))}
        </select>
      </label>
      <label>
        <span>Speed</span>
        <select
          value={form.serviceTier}
          onChange={(event) => onChange({ ...form, serviceTier: event.target.value })}
        >
          {CODEX_SERVICE_TIERS.map((tier) => (
            <option key={tier.value || "default"} value={tier.value}>{tier.label}</option>
          ))}
        </select>
      </label>
    </>
  );

  return (
    <Modal open={open} title={title} onClose={onCancel} width={700}>
      <div className="modal-form agent-modal-form">
        {createMode ? (
          <>
            <div className="agent-form-preview">
              <AgentAvatar agent={previewAgent} size="lg" showStatus={false} />
              <div>
                <strong>{previewName}</strong>
                <span>{modelLabel(form.model)} · @{previewHandle}</span>
              </div>
            </div>
            <div className="two-col">
              <label>
                <span>Name</span>
                <input
                  autoFocus
                  value={form.displayName}
                  onChange={(event) => onChange({ ...form, displayName: event.target.value })}
                  placeholder="Agent name"
                />
              </label>
              {runtimeSelect}
            </div>
            <div className={isCodex ? "three-col" : "two-col"}>
              {modelSelect}
              {codexControls}
            </div>
            <label>
              <span>Avatar</span>
              <AvatarInput
                value={form.avatar}
                seedHint={seedForAgentForm(form)}
                onChange={(avatar) => onChange({ ...form, avatar })}
              />
            </label>
            <RuntimePreflight check={runtimeChecks[form.runtime]} />
            <details className="agent-advanced-settings">
              <summary>Advanced</summary>
              <div className="two-col">
                <label>
                  <span>Mention handle</span>
                  <input
                    value={form.handle}
                    onChange={(event) => onChange({ ...form, handle: event.target.value })}
                    placeholder="auto from name"
                  />
                </label>
                <label>
                  <span>Role</span>
                  <input
                    value={form.role}
                    onChange={(event) => onChange({ ...form, role: event.target.value })}
                    placeholder="agent"
                  />
                </label>
              </div>
              <label>
                <span>Daily budget</span>
                <input
                  value={form.dailyBudgetUsd}
                  inputMode="decimal"
                  onChange={(event) => onChange({ ...form, dailyBudgetUsd: event.target.value })}
                  placeholder="USD per day"
                />
              </label>
              <label>
                <span>Workspace directory</span>
                <input
                  value={form.workingDirectory}
                  onChange={(event) => onChange({ ...form, workingDirectory: event.target.value })}
                  placeholder="~/Library/Application Support/Lantor/agents/<handle>"
                />
              </label>
            </details>
          </>
        ) : (
          <>
            <div className="two-col">
              <label>
                <span>Mention handle</span>
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
              {runtimeSelect}
              {modelSelect}
            </div>
            {isCodex && <div className="two-col">{codexControls}</div>}
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
                <AvatarInput
                  value={form.avatar}
                  seedHint={seedForAgentForm(form)}
                  onChange={(avatar) => onChange({ ...form, avatar })}
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
          </>
        )}
        <div className="modal-actions">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!canSubmit} onClick={onSubmit}>{submitLabel}</button>
        </div>
      </div>
    </Modal>
  );
}
