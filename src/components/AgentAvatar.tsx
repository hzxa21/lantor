import { useEffect, useState, type CSSProperties } from "react";
import type { Style } from "@dicebear/core";
import type { Agent } from "../types";

type AgentAvatarProps = {
  agent: Pick<Agent, "handle" | "display_name" | "status"> &
    Partial<Pick<Agent, "id" | "runtime" | "model" | "role" | "avatar" | "description">>;
  size?: "sm" | "md" | "lg";
  className?: string;
  title?: string;
};

type AgentAvatarWithProfileProps = AgentAvatarProps & {
  clickHint?: string;
};

const IDENTICON_SIZE = 5;
const IDENTICON_MIRROR_WIDTH = Math.ceil(IDENTICON_SIZE / 2);
const DEFAULT_DICEBEAR_STYLE = "bottts-neutral";
const DICEBEAR_STYLE_LOADERS = {
  adventurer: () => import("@dicebear/adventurer"),
  "bottts-neutral": () => import("@dicebear/bottts-neutral"),
  identicon: () => import("@dicebear/identicon"),
  initials: () => import("@dicebear/initials"),
  lorelei: () => import("@dicebear/lorelei"),
  notionists: () => import("@dicebear/notionists"),
  personas: () => import("@dicebear/personas"),
  "pixel-art": () => import("@dicebear/pixel-art"),
  shapes: () => import("@dicebear/shapes"),
} as const;
type DiceBearStyleName = keyof typeof DICEBEAR_STYLE_LOADERS;
type DiceBearStyle = Style<Record<string, unknown>>;

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

function normalizeDiceBearStyle(style: string) {
  const normalized = style
    .trim()
    .replace(/([a-z0-9])([A-Z])/g, "$1-$2")
    .replace(/[_\s]+/g, "-")
    .toLowerCase();
  return normalized in DICEBEAR_STYLE_LOADERS
    ? (normalized as DiceBearStyleName)
    : DEFAULT_DICEBEAR_STYLE;
}

function parseDiceBearAvatar(value: string, fallbackSeed: string) {
  const trimmed = value.trim();
  const lower = trimmed.toLowerCase();
  if (lower !== "dicebear" && !lower.startsWith("dicebear:")) return null;
  const [, rawStyle = DEFAULT_DICEBEAR_STYLE, ...seedParts] = trimmed.split(":");
  const style = normalizeDiceBearStyle(rawStyle || DEFAULT_DICEBEAR_STYLE);
  const seed = seedParts.join(":").trim() || fallbackSeed;
  return { style, seed };
}

function isImageAvatar(value: string) {
  return /^https?:\/\//i.test(value) || /^data:image\//i.test(value);
}

function compactProfileText(value: string | null | undefined, fallback: string) {
  const trimmed = value?.trim();
  if (!trimmed) return fallback;
  return trimmed.length > 110 ? `${trimmed.slice(0, 107).trimEnd()}...` : trimmed;
}

function statusLabel(status: string) {
  return status.replace(/_/g, " ");
}

export function AgentAvatar({ agent, size = "md", className = "", title }: AgentAvatarProps) {
  const seedText = agent.id || `${agent.handle}:${agent.display_name}:${agent.runtime ?? ""}`;
  const identicon = generateIdenticon(seedText);
  const customAvatar = agent.avatar?.trim();
  const diceBearSpec = customAvatar ? parseDiceBearAvatar(customAvatar, seedText) : null;
  const [diceBearAvatar, setDiceBearAvatar] = useState<string | null>(null);
  const style = {
    "--avatar-color": identicon.foreground,
    "--avatar-bg": identicon.background,
  } as CSSProperties;

  useEffect(() => {
    if (!diceBearSpec) {
      setDiceBearAvatar(null);
      return;
    }

    let cancelled = false;
    setDiceBearAvatar(null);
    Promise.all([
      import("@dicebear/core"),
      DICEBEAR_STYLE_LOADERS[diceBearSpec.style](),
    ])
      .then(([{ createAvatar }, styleDefinition]) => {
        if (cancelled) return;
        const avatar = createAvatar(styleDefinition as unknown as DiceBearStyle, {
          seed: diceBearSpec.seed,
        });
        setDiceBearAvatar(avatar.toDataUri());
      })
      .catch(() => {
        if (!cancelled) setDiceBearAvatar(null);
      });

    return () => {
      cancelled = true;
    };
  }, [diceBearSpec?.seed, diceBearSpec?.style]);

  return (
    <span
      className={`avatar agent-avatar agent-avatar-${size} status-${agent.status} ${className}`.trim()}
      style={style}
      title={title}
      aria-hidden={!title}
    >
      {diceBearAvatar ? (
        <img className="agent-avatar-image" src={diceBearAvatar} alt="" aria-hidden="true" />
      ) : customAvatar && !diceBearSpec && isImageAvatar(customAvatar) ? (
        <img className="agent-avatar-image" src={customAvatar} alt="" aria-hidden="true" />
      ) : customAvatar && !diceBearSpec ? (
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

export function AgentAvatarWithProfile({
  agent,
  size = "md",
  className = "",
  clickHint = "Click to open details",
}: AgentAvatarWithProfileProps) {
  const role = compactProfileText(agent.role, `${agent.runtime ?? "agent"} agent`);
  const description = compactProfileText(agent.description, "No profile description yet.");
  const runtimeModel = [agent.runtime, agent.model].filter(Boolean).join(" / ");

  return (
    <span className="agent-avatar-profile-anchor">
      <AgentAvatar agent={agent} size={size} className={className} />
      <span className="agent-avatar-profile-card" aria-hidden="true">
        <span className="agent-avatar-profile-name">{agent.display_name}</span>
        <span className="agent-avatar-profile-handle">@{agent.handle}</span>
        <span className="agent-avatar-profile-role">{role}</span>
        <span className="agent-avatar-profile-description">{description}</span>
        <span className="agent-avatar-profile-meta">
          {runtimeModel || "runtime unknown"} · {statusLabel(agent.status)}
        </span>
        <span className="agent-avatar-profile-hint">{clickHint}</span>
      </span>
    </span>
  );
}
