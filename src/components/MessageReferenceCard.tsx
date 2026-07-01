import { Hash, MessageSquare, PanelRightOpen, X } from "lucide-react";
import type { KeyboardEvent, MouseEvent } from "react";
import type { Channel, Message } from "../types";
import { formatTime } from "../ui-utils";
import {
  type ResolvedMessageReference,
  parseMessageReferences,
  resolveMessageReference,
} from "../message-references";

type MessageReferenceCardProps = {
  reference: ResolvedMessageReference;
  compact?: boolean;
  removable?: boolean;
  onOpen?: (reference: ResolvedMessageReference) => void;
  onRemove?: (token: string) => void;
};

export function firstLinePreview(body: string, limit = 140) {
  const normalized = body.replace(/\s+/g, " ").trim();
  if (!normalized) return "Empty message";
  return normalized.length > limit ? `${normalized.slice(0, limit - 1).trimEnd()}...` : normalized;
}

function referenceLabel(reference: ResolvedMessageReference) {
  return reference.kind === "thread" ? "Thread" : "Message";
}

function referenceMeta(reference: ResolvedMessageReference) {
  const parts = [
    reference.channel ? `#${reference.channel.name}` : "Unknown channel",
    reference.message?.sender_name ?? "Unknown sender",
  ];
  if (reference.kind === "thread" && reference.replyCount !== null) {
    parts.push(`${reference.replyCount} ${reference.replyCount === 1 ? "reply" : "replies"}`);
  }
  if (reference.message) parts.push(formatTime(reference.message.created_at));
  return parts.join(" · ");
}

export function MessageReferenceCard({
  reference,
  compact = false,
  removable = false,
  onOpen,
  onRemove,
}: MessageReferenceCardProps) {
  const Icon = reference.kind === "thread" ? PanelRightOpen : MessageSquare;
  const preview = reference.message
    ? firstLinePreview(reference.message.body, compact ? 96 : 150)
    : `Missing ${reference.kind} ${reference.id.slice(0, 8)}`;
  const className = [
    "message-reference-card",
    reference.kind,
    compact ? "compact" : "",
    reference.message ? "" : "missing",
  ].filter(Boolean).join(" ");
  const openProps = {
    role: onOpen ? "button" : undefined,
    tabIndex: onOpen ? 0 : undefined,
    onClick: (event: MouseEvent) => {
      event.stopPropagation();
      onOpen?.(reference);
    },
    onKeyDown: (event: KeyboardEvent) => {
      if (!onOpen) return;
      if (event.key !== "Enter" && event.key !== " ") return;
      event.preventDefault();
      onOpen(reference);
    },
  };

  if (compact) {
    const channelLabel = reference.channel ? `#${reference.channel.name}` : "Unknown channel";
    const senderLabel = reference.message?.sender_name ?? "Unknown";
    return (
      <span
        className={className}
        {...openProps}
        title={reference.message ? preview : reference.token}
      >
        <span className="message-reference-icon" aria-hidden="true">
          <Icon size={13} />
        </span>
        <span className="message-reference-inline-copy">
          <strong>{referenceLabel(reference)}</strong>
          <span>{channelLabel} · {senderLabel}</span>
        </span>
      </span>
    );
  }

  return (
    <span
      className={className}
      {...openProps}
      title={reference.message ? preview : reference.token}
    >
      <span className="message-reference-icon" aria-hidden="true">
        <Icon size={compact ? 15 : 16} />
      </span>
      <span className="message-reference-copy">
        <span className="message-reference-kicker">
          <strong>{referenceLabel(reference)}</strong>
          <span>{referenceMeta(reference)}</span>
        </span>
        <span className="message-reference-preview">{preview}</span>
      </span>
      {reference.channel && (
        <span className="message-reference-channel" aria-hidden="true">
          <Hash size={12} />
        </span>
      )}
      {removable && (
        <button
          type="button"
          className="message-reference-remove"
          aria-label={`Remove ${reference.kind} reference`}
          onClick={(event) => {
            event.stopPropagation();
            onRemove?.(reference.token);
          }}
        >
          <X size={14} />
        </button>
      )}
    </span>
  );
}

type MessageReferencePreviewProps = {
  text: string;
  messages: Message[];
  channels: Channel[];
  onOpen?: (reference: ResolvedMessageReference) => void;
  onRemove?: (token: string) => void;
};

export function MessageReferencePreview({
  text,
  messages,
  channels,
  onOpen,
  onRemove,
}: MessageReferencePreviewProps) {
  const references = parseMessageReferences(text).map((reference) => resolveMessageReference(reference, messages, channels));
  if (references.length === 0) return null;
  return (
    <div className="message-reference-preview-list" aria-label="Message references">
      {references.map((reference) => (
        <MessageReferenceCard
          key={`${reference.kind}:${reference.id}`}
          reference={reference}
          removable={Boolean(onRemove)}
          onOpen={onOpen}
          onRemove={onRemove}
        />
      ))}
    </div>
  );
}
