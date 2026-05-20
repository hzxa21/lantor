import { Agent } from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { Modal } from "./Modal";

type CreateChannelModalProps = {
  open: boolean;
  channelName: string;
  agents: Agent[];
  selectedAgentIds: Set<string>;
  onChange: (value: string) => void;
  onToggleAgent: (agentId: string, member: boolean) => void;
  onCancel: () => void;
  onSubmit: () => void;
};

export function CreateChannelModal({
  open,
  channelName,
  agents,
  selectedAgentIds,
  onChange,
  onToggleAgent,
  onCancel,
  onSubmit,
}: CreateChannelModalProps) {
  return (
    <Modal
      open={open}
      title="Create Channel"
      onClose={onCancel}
      width={560}
      closeOnBackdrop={false}
      closeOnEscape={false}
    >
      <div className="modal-form">
        <label>
          <span>Channel name</span>
          <input
            autoFocus
            autoCapitalize="none"
            autoComplete="off"
            autoCorrect="off"
            name="lantor-channel-name"
            spellCheck={false}
            value={channelName}
            onChange={(event) => onChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") onSubmit();
            }}
            placeholder="lantor"
          />
        </label>
        {agents.length > 0 && (
          <label>
            <span>Invite agents</span>
            <div className="member-editor modal-member-editor channel-agent-picker">
              {agents.map((agent) => {
                const isMember = selectedAgentIds.has(agent.id);
                return (
                  <label key={agent.id} className={`channel-agent-option ${isMember ? "selected" : ""}`}>
                    <input
                      className="channel-agent-checkbox"
                      type="checkbox"
                      checked={isMember}
                      onChange={(event) => onToggleAgent(agent.id, event.target.checked)}
                      aria-label={`${isMember ? "Remove" : "Add"} @${agent.handle}`}
                    />
                    <div className="channel-agent-profile">
                      <AgentAvatar agent={agent} size="sm" title={`@${agent.handle}`} />
                      <div className="agent-pick-row">
                        <strong>{agent.display_name}</strong>
                        <small>@{agent.handle}</small>
                      </div>
                    </div>
                  </label>
                );
              })}
            </div>
          </label>
        )}
        <div className="modal-actions">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!channelName.trim()} onClick={onSubmit}>Create</button>
        </div>
      </div>
    </Modal>
  );
}
