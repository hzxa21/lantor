import type { CSSProperties } from "react";
import type { Agent } from "../types";

type AgentAvatarProps = {
  agent: Pick<Agent, "handle" | "display_name" | "status"> & Partial<Pick<Agent, "id" | "runtime" | "avatar">>;
  size?: "sm" | "md" | "lg";
  className?: string;
  title?: string;
};

const IDENTICON_SIZE = 5;
const IDENTICON_MIRROR_WIDTH = Math.ceil(IDENTICON_SIZE / 2);

function hashSeed(seed: string) {
  let hash = 2166136261;
  for (let index = 0; index < seed.length; index += 1) {
    hash ^= seed.charCodeAt(index);
    hash = Math.imul(hash, 16777619);
  }
  return hash >>> 0;
}

function nextSeed(seed: number) {
  let next = seed + 0x6d2b79f5;
  next = Math.imul(next ^ (next >>> 15), next | 1);
  next ^= next + Math.imul(next ^ (next >>> 7), next | 61);
  return (next ^ (next >>> 14)) >>> 0;
}

function generateIdenticon(seedText: string) {
  let seed = hashSeed(seedText);
  const cells = Array.from({ length: IDENTICON_SIZE * IDENTICON_SIZE }, () => false);

  for (let y = 0; y < IDENTICON_SIZE; y += 1) {
    for (let x = 0; x < IDENTICON_MIRROR_WIDTH; x += 1) {
      seed = nextSeed(seed);
      const isFilled = seed % 100 < 58;
      cells[y * IDENTICON_SIZE + x] = isFilled;
      cells[y * IDENTICON_SIZE + (IDENTICON_SIZE - 1 - x)] = isFilled;
    }
  }

  const colorSeed = hashSeed(`${seedText}:color`);
  const hue = colorSeed % 360;
  return {
    cells,
    foreground: `hsl(${hue} 68% 42%)`,
    background: `hsl(${hue} 36% 94%)`,
  };
}

export function AgentAvatar({ agent, size = "md", className = "", title }: AgentAvatarProps) {
  const seedText = agent.id || `${agent.handle}:${agent.display_name}:${agent.runtime ?? ""}`;
  const identicon = generateIdenticon(seedText);
  const customAvatar = agent.avatar?.trim();
  const style = {
    "--avatar-color": identicon.foreground,
    "--avatar-bg": identicon.background,
  } as CSSProperties;

  return (
    <span
      className={`avatar agent-avatar agent-avatar-${size} status-${agent.status} ${className}`.trim()}
      style={style}
      title={title}
      aria-hidden={!title}
    >
      {customAvatar ? (
        <span className="agent-avatar-glyph" aria-hidden="true">{customAvatar.slice(0, 2)}</span>
      ) : (
        <span className="agent-avatar-pixels" aria-hidden="true">
          {identicon.cells.map((filled, index) => (
            <span key={index} className={filled ? "filled" : ""} />
          ))}
        </span>
      )}
    </span>
  );
}
