import { Shuffle } from "lucide-react";
import { randomDylanAvatarSpec } from "../avatar-utils";

type AvatarInputProps = {
  value: string;
  onChange: (value: string) => void;
  seedHint: string;
};

export function AvatarInput({ value, onChange, seedHint }: AvatarInputProps) {
  return (
    <div className="avatar-input">
      <input
        value={value}
        onChange={(event) => onChange(event.target.value)}
        placeholder="emoji, initials, URL, or dicebear:dylan"
      />
      <button
        type="button"
        title="Random Dylan avatar"
        aria-label="Random Dylan avatar"
        onClick={() => onChange(randomDylanAvatarSpec(seedHint))}
      >
        <Shuffle size={16} />
      </button>
    </div>
  );
}
