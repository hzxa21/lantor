import type { CSSProperties } from "react";
import type { Agent } from "../types";

type AgentAvatarProps = {
  agent: Pick<Agent, "handle" | "display_name" | "avatar" | "status"> & Partial<Pick<Agent, "runtime">>;
  size?: "sm" | "md" | "lg";
  className?: string;
  title?: string;
};

const PALETTES = [
  ["#0a84ff", "#64d2ff", "#04395e"],
  ["#ff7a18", "#ffd166", "#7a2e00"],
  ["#30d158", "#b8f7c5", "#06451c"],
  ["#ff375f", "#ff9fbd", "#5b1023"],
  ["#5e5ce6", "#bfbcff", "#1d1b5f"],
  ["#00c7be", "#9ff7ef", "#004743"],
  ["#af52de", "#f0b5ff", "#4b1065"],
  ["#8e8e93", "#f2f2f7", "#1c1c1e"],
];

function hashSeed(seed: string) {
  let hash = 2166136261;
  for (let index = 0; index < seed.length; index += 1) {
    hash ^= seed.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function initials(agent: AgentAvatarProps["agent"]) {
  if (agent.avatar.trim()) return agent.avatar.trim().slice(0, 2);
  const parts = (agent.display_name || agent.handle)
    .replace(/^@/, "")
    .split(/[\s._-]+/)
    .filter(Boolean);
  const letters = parts.length > 1
    ? `${parts[0][0]}${parts[1][0]}`
    : (parts[0] ?? agent.handle).slice(0, 2);
  return letters.toUpperCase();
}

export function AgentAvatar({ agent, size = "md", className = "", title }: AgentAvatarProps) {
  const seed = hashSeed(`${agent.handle}:${agent.display_name}:${agent.runtime ?? ""}`);
  const palette = PALETTES[seed % PALETTES.length];
  const style = {
    "--avatar-a": palette[0],
    "--avatar-b": palette[1],
    "--avatar-c": palette[2],
    "--avatar-rotate": `${seed % 360}deg`,
    "--avatar-x": `${18 + (seed % 48)}%`,
    "--avatar-y": `${16 + ((seed >> 8) % 48)}%`,
  } as CSSProperties;

  return (
    <span
      className={`avatar agent-avatar agent-avatar-${size} status-${agent.status} ${className}`.trim()}
      style={style}
      title={title}
      aria-hidden={!title}
    >
      <span className="agent-avatar-mark">{initials(agent)}</span>
    </span>
  );
}
