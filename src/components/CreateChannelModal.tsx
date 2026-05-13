import { Modal } from "./Modal";

type CreateChannelModalProps = {
  open: boolean;
  channelName: string;
  onChange: (value: string) => void;
  onCancel: () => void;
  onSubmit: () => void;
};

export function CreateChannelModal({
  open,
  channelName,
  onChange,
  onCancel,
  onSubmit,
}: CreateChannelModalProps) {
  return (
    <Modal open={open} title="Create Channel" onClose={onCancel}>
      <div className="modal-form">
        <label>
          <span>Channel name</span>
          <input
            autoFocus
            value={channelName}
            onChange={(event) => onChange(event.target.value)}
            onKeyDown={(event) => {
              if (event.key === "Enter") onSubmit();
            }}
            placeholder="lantor"
          />
        </label>
        <div className="modal-actions">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!channelName.trim()} onClick={onSubmit}>Create</button>
        </div>
      </div>
    </Modal>
  );
}
