import { Bell, Check, Hash, Inbox, MessageSquare, UserRound, X } from "lucide-react";
import { useEffect, useMemo, useRef, useState } from "react";
import type { CSSProperties, PointerEvent } from "react";
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
  onDismissItem: (item: InboxItem) => void;
  onDismissItems: (items: InboxItem[]) => void;
  onMarkAllRead: (items: InboxItem[]) => void;
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

const SWIPE_DISMISS_THRESHOLD_PX = 86;
const SWIPE_REVEAL_MAX_PX = 96;

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
  onDismissItem,
  onDismissItems,
  onMarkAllRead,
  onClose,
}: InboxModalProps) {
  const [filter, setFilter] = useState<InboxFilter>("all");
  const [swipeState, setSwipeState] = useState<{
    itemId: string;
    startX: number;
    startY: number;
    offsetX: number;
    tracking: boolean;
  } | null>(null);
  const suppressNextClickRef = useRef(false);
  const unreadCount = items.filter((item) => item.unread).length;
  const filteredItems = useMemo(() => {
    if (filter === "all") return items;
    if (filter === "unread") return items.filter((item) => item.unread);
    return items.filter((item) => item.kind === filter);
  }, [filter, items]);
  const filteredUnreadCount = filteredItems.filter((item) => item.unread).length;

  useEffect(() => {
    if (!open) return;
    function onKey(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [open, onClose]);

  if (!open) return null;

  function startSwipe(item: InboxItem, event: PointerEvent<HTMLElement>) {
    if (event.pointerType === "mouse") return;
    if ((event.target as HTMLElement).closest("button")) return;
    event.currentTarget.setPointerCapture?.(event.pointerId);
    setSwipeState({
      itemId: item.id,
      startX: event.clientX,
      startY: event.clientY,
      offsetX: 0,
      tracking: true,
    });
  }

  function moveSwipe(item: InboxItem, event: PointerEvent<HTMLElement>) {
    setSwipeState((current) => {
      if (!current || current.itemId !== item.id || !current.tracking) return current;
      const deltaX = event.clientX - current.startX;
      const deltaY = event.clientY - current.startY;
      if (Math.abs(deltaY) > Math.abs(deltaX) && Math.abs(deltaY) > 12) {
        return { ...current, tracking: false, offsetX: 0 };
      }
      if (deltaX >= 0) return { ...current, offsetX: 0 };
      event.preventDefault();
      return { ...current, offsetX: Math.max(-SWIPE_REVEAL_MAX_PX, deltaX) };
    });
  }

  function endSwipe(item: InboxItem) {
    const current = swipeState;
    setSwipeState(null);
    if (!current || current.itemId !== item.id) return;
    if (Math.abs(current.offsetX) > 8) {
      suppressNextClickRef.current = true;
      window.setTimeout(() => {
        suppressNextClickRef.current = false;
      }, 0);
    }
    if (current.offsetX <= -SWIPE_DISMISS_THRESHOLD_PX) {
      onDismissItem(item);
    }
  }

  function openItem(item: InboxItem) {
    if (suppressNextClickRef.current) {
      suppressNextClickRef.current = false;
      return;
    }
    onOpenItem(item);
  }

  return (
    <div className="search-backdrop" onClick={onClose}>
      <section className="inbox-panel" onClick={(event) => event.stopPropagation()}>
        <header className="inbox-head">
          <button className="inbox-back" onClick={onClose} aria-label="Close inbox">
            <X size={18} />
          </button>
          <div>
            <h2>Activity</h2>
            <p>{items.length} active · {unreadCount} unread</p>
          </div>
          <div className="inbox-head-actions">
            <button
              className="inbox-mark-all"
              disabled={filteredUnreadCount === 0}
              onClick={() => onMarkAllRead(filteredItems)}
            >
              Mark all read
            </button>
            <button
              className="inbox-dismiss-all"
              disabled={filteredItems.length === 0}
              onClick={() => onDismissItems(filteredItems)}
            >
              Dismiss all
            </button>
          </div>
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
            const swipeOffset = swipeState?.itemId === item.id ? swipeState.offsetX : 0;
            return (
              <div
                key={item.id}
                className={`inbox-row-shell ${swipeOffset < 0 ? "swiping" : ""}`}
                style={{ "--inbox-swipe-x": `${swipeOffset}px` } as CSSProperties}
                onPointerDown={(event) => startSwipe(item, event)}
                onPointerMove={(event) => moveSwipe(item, event)}
                onPointerUp={() => endSwipe(item)}
                onPointerCancel={() => setSwipeState(null)}
              >
                <div className="inbox-swipe-action" aria-hidden="true">
                  <X size={18} />
                  <span>Dismiss</span>
                </div>
                <article
                  className={`inbox-row ${item.unread ? "unread" : ""}`}
                  onClick={() => openItem(item)}
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
                    <small>
                      Open
                      {item.newCount > 0 ? <b>{item.newCount} new</b> : null}
                    </small>
                  </div>
                  <div className="inbox-row-actions">
                    {item.unread ? <span className="inbox-unread-dot" aria-label="Unread" /> : null}
                    {item.unread ? (
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
                    ) : null}
                    <button
                      className="inbox-dismiss"
                      title="Dismiss"
                      onClick={(event) => {
                        event.stopPropagation();
                        onDismissItem(item);
                      }}
                    >
                      <X size={18} />
                    </button>
                  </div>
                </article>
              </div>
            );
          })}
        </div>
      </section>
    </div>
  );
}
