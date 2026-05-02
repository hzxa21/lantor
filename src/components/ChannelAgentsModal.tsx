import { Agent, Channel } from "../types";
import { Modal } from "./Modal";

type ChannelAgentsModalProps = {
  open: boolean;
  channel: Channel | null;
  agents: Agent[];
  channelMemberIds: Set<string>;
  onSetMember: (agentId: string, member: boolean) => void;
  onCreateAgent: () => void;
  onClose: () => void;
};

export function ChannelAgentsModal({
  open,
  channel,
  agents,
  channelMemberIds,
  onSetMember,
  onCreateAgent,
  onClose,
}: ChannelAgentsModalProps) {
  return (
    <Modal
      open={open}
      title={channel ? `Agents in #${channel.name}` : "Channel Agents"}
      onClose={onClose}
      width={560}
    >
      <div className="modal-form">
        <div className="member-editor modal-member-editor channel-agent-picker">
          {agents.length === 0 && <span>No agents yet. Create an agent first.</span>}
          {agents.map((agent) => (
            <label key={agent.id}>
              <input
                type="checkbox"
                checked={channelMemberIds.has(agent.id)}
                onChange={(event) => onSetMember(agent.id, event.target.checked)}
              />
              <span className="agent-pick-row">
                <strong>@{agent.handle}</strong>
                <small>{agent.display_name} · {agent.runtime} · {agent.status}</small>
              </span>
            </label>
          ))}
        </div>
        <div className="modal-actions split">
          <button onClick={onCreateAgent}>Create new agent</button>
          <button className="primary" onClick={onClose}>Done</button>
        </div>
      </div>
    </Modal>
  );
}
