import { useMemo } from "react";
import type { OwnerProfile } from "../types";
import { AgentAvatar } from "./AgentAvatar";
import { AvatarInput } from "./AvatarInput";
import { Modal } from "./Modal";

export type OwnerProfileForm = {
  displayName: string;
  avatar: string;
  description: string;
};

type OwnerProfileModalProps = {
  open: boolean;
  form: OwnerProfileForm;
  onChange: (form: OwnerProfileForm) => void;
  onCancel: () => void;
  onSubmit: () => void;
};

export function ownerProfileToForm(profile: OwnerProfile): OwnerProfileForm {
  return {
    displayName: profile.display_name,
    avatar: profile.avatar,
    description: profile.description,
  };
}

function seedForProfile(form: OwnerProfileForm) {
  const seed = [form.displayName, form.description]
    .map((value) => value.trim())
    .filter(Boolean)
    .join(":");
  return seed || "owner";
}

export function OwnerProfileModal({
  open,
  form,
  onChange,
  onCancel,
  onSubmit,
}: OwnerProfileModalProps) {
  const previewAgent = useMemo(() => ({
    id: "owner-profile",
    handle: "owner",
    display_name: form.displayName.trim() || "Owner",
    status: "idle",
    avatar: form.avatar,
    description: form.description,
  }), [form.avatar, form.description, form.displayName]);

  return (
    <Modal open={open} title="Edit Profile" onClose={onCancel} width={560}>
      <div className="modal-form owner-profile-form">
        <div className="owner-profile-preview">
          <AgentAvatar agent={previewAgent} size="lg" showStatus={false} />
          <div>
            <strong>{previewAgent.display_name}</strong>
            <span>{form.description.trim() || "local owner"}</span>
          </div>
        </div>
        <div className="two-col">
          <label>
            <span>Name</span>
            <input
              autoFocus
              value={form.displayName}
              onChange={(event) => onChange({ ...form, displayName: event.target.value })}
              placeholder="Display name"
            />
          </label>
          <label>
            <span>Avatar</span>
            <AvatarInput
              value={form.avatar}
              seedHint={seedForProfile(form)}
              onChange={(avatar) => onChange({ ...form, avatar })}
            />
          </label>
        </div>
        <label>
          <span>Description</span>
          <textarea
            value={form.description}
            onChange={(event) => onChange({ ...form, description: event.target.value })}
            placeholder="Short personal description"
          />
        </label>
        <div className="modal-actions">
          <button onClick={onCancel}>Cancel</button>
          <button className="primary" disabled={!form.displayName.trim()} onClick={onSubmit}>Save</button>
        </div>
      </div>
    </Modal>
  );
}
