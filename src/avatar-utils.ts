const DYLAN_DICEBEAR_STYLE = "dylan";

function randomToken() {
  if (typeof globalThis.crypto?.randomUUID === "function") {
    return globalThis.crypto.randomUUID();
  }
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2, 12)}`;
}

function seedPrefix(value: string) {
  return value
    .trim()
    .replace(/[^A-Za-z0-9_-]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 32)
    .toLowerCase();
}

export function randomDylanAvatarSpec(seedHint = "avatar") {
  const prefix = seedPrefix(seedHint) || "avatar";
  return `dicebear:${DYLAN_DICEBEAR_STYLE}:${prefix}-${randomToken()}`;
}
