import type { Message } from "./types";

const COMPACT_FOLLOWUP_WINDOW_MS = 5 * 60 * 1000;

export function isStreamingMessage(message: Message) {
  return message.delivery_state === "streaming";
}

export function isCompactFollowupMessage(message: Message, previous: Message | null | undefined) {
  if (!previous) return false;
  if (message.sender_role === "system" || previous.sender_role === "system") return false;
  if (message.is_task) return false;
  if (message.delivery_state !== "complete") return false;
  if (message.sender_name !== previous.sender_name || message.sender_role !== previous.sender_role) return false;

  const createdAt = new Date(message.created_at).getTime();
  const previousCreatedAt = new Date(previous.created_at).getTime();
  if (!Number.isFinite(createdAt) || !Number.isFinite(previousCreatedAt)) return false;
  const updatedAt = new Date(message.updated_at).getTime();
  if (Number.isFinite(updatedAt) && updatedAt - createdAt > 1000) return false;

  const delta = createdAt - previousCreatedAt;
  return delta >= 0 && delta <= COMPACT_FOLLOWUP_WINDOW_MS;
}
