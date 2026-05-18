import { Bell, Check, ChevronRight, Hash, Inbox, MessageSquare, UserRound, X } from "lucide-react";
import { useEffect, useMemo, useState } from "react";
import type { Agent, InboxItem, InboxKind, OwnerProfile } from "../types";
import { firstLines, formatTime, ownerAsAvatarAgent } from "../ui-utils";
import { AgentAvatar } from "./AgentAvatar";

type InboxFilter = "all" | "unread" | InboxKind;

type InboxModalProps = {
  open: boolean;
  items: InboxItem[];
  agents: Agent[];
  ownerProfile: OwnerProfile;
  onOpenItem: (item: InboxItem) => void;
  onMarkItemRead: (item: InboxItem) => void;
  onMarkAllRead: () => void;
  onClose: () => void;
};

const FILTERS: { value: InboxFilter; label: string }[] = [
  { value: "all", label: "All" },
  { value: "unread", label: "Unread" },
  { value: "mention", label: "Mentions" },
  { value: "dm", label: "DMs" },
  { value: "thread", label: "Threads" },
  { value: "task", label: "Tasks" },
  { value: "reminder", label: "Reminders" },
];

function iconFor(kind: InboxKind) {
  if (kind === "reminder") return Bell;
  if (kind === "dm") return UserRound;
  if (kind === "thread" || kind === "mention") return MessageSquare;
  return Hash;
}

function kindLabel(kind: InboxKind) {
  return kind === "dm" ? "DM" : kind;
}

function actorAvatarAgent(item: InboxItem, agents: Agent[], ownerProfile: OwnerProfile) {
  if (item.actorAgentId) return agents.find((agent) => agent.id === item.actorAgentId) ?? null;
  if (item.actorRole === "owner") return ownerAsAvatarAgent(ownerProfile);
  return null;
}

export function InboxModal({
  open,
  items,
  agents,
  ownerProfile,
  onOpenItem,
  onMarkItemRead,
  onMarkAllRead,
  onClose,
}: InboxModalProps) {
  const [filter, setFilter] = useState<InboxFilter>("all");
  const unreadCount = items.filter((item) => item.unread).length;
  const filteredItems = useMemo(() => {
    if (filter === "all") return items;
    if (filter === "unread") return items.filter((item) => item.unread);
    return items.filter((item) => item.kind === filter);
  }, [filter, items]);

  useEffect(() => {
    if (!open) return;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  return (
    <div className="search-backdrop" onClick={onClose}>
      <section className="inbox-panel" onClick={(event) => event.stopPropagation()}>
        <header className="inbox-head">
          <button className="inbox-back" onClick={onClose} aria-label="Close inbox">
            <X size={18} />
          </button>
          <div>
            <h2>Inbox</h2>
            <p>{items.length} active · {unreadCount} unread</p>
          </div>
          <button className="inbox-mark-all" disabled={unreadCount === 0} onClick={onMarkAllRead}>
            Mark all read
          </button>
        </header>

        <div className="inbox-filters">
          {FILTERS.map((item) => (
            <button
              key={item.value}
              className={filter === item.value ? "active" : ""}
              onClick={() => setFilter(item.value)}
            >
              {item.label}
            </button>
          ))}
        </div>

        <div className="inbox-body">
          {filteredItems.length === 0 && (
            <div className="search-empty">
              <Inbox size={34} />
              <h3>No inbox items</h3>
              <p>Mentions, DMs, followed thread updates, active tasks, and due reminders will appear here.</p>
            </div>
          )}

          {filteredItems.map((item) => {
            const Icon = iconFor(item.kind);
            const avatarAgent = actorAvatarAgent(item, agents, ownerProfile);
            return (
              <article
                key={item.id}
                className={`inbox-row ${item.unread ? "unread" : ""}`}
                onClick={() => onOpenItem(item)}
              >
                <span className="inbox-row-avatar" aria-hidden="true">
                  {avatarAgent ? (
                    <AgentAvatar agent={avatarAgent} size="md" showStatus={false} />
                  ) : (
                    <span className="search-result-fallback-avatar">{item.actor?.slice(0, 1) || kindLabel(item.kind).slice(0, 1)}</span>
                  )}
                </span>
                <div className="inbox-row-main">
                  <div className="inbox-row-meta">
                    {item.actor && <strong>{item.actor}</strong>}
                    <span>{item.surface}</span>
                    <time>{formatTime(item.timestamp)}</time>
                    <em>{kindLabel(item.kind)}</em>
                  </div>
                  <h3>
                    <Icon size={18} />
                    {item.title}
                  </h3>
                  {item.excerpt && <p>{firstLines(item.excerpt, 2)}</p>}
                  {item.replyCount > 0 ? (
                    <div className="inbox-reply-summary thread-reply-summary">
                      <div className="thread-reply-avatars inbox-reply-icon" aria-hidden="true">
                        <span>
                          <MessageSquare size={13} />
                        </span>
                      </div>
                      <strong>{item.replyCount} {item.replyCount === 1 ? "reply" : "replies"}</strong>
                      <span className="thread-reply-summary-action">
                        <time dateTime={item.timestamp}>Last reply {formatTime(item.timestamp)}</time>
                        <span className="thread-reply-summary-open">Open thread</span>
                      </span>
                      {item.newCount > 0 ? <span className="inbox-new-count">{item.newCount} new</span> : null}
                      <ChevronRight className="thread-reply-summary-icon" size={18} aria-hidden="true" />
                    </div>
                  ) : (
                    <small>
                      Open
                      {item.newCount > 0 ? <b>{item.newCount} new</b> : null}
                    </small>
                  )}
                </div>
                <button
                  className="inbox-check"
                  title="Mark read"
                  onClick={(event) => {
                    event.stopPropagation();
                    onMarkItemRead(item);
                  }}
                >
                  <Check size={19} />
                </button>
              </article>
            );
          })}
        </div>
      </section>
    </div>
  );
}
