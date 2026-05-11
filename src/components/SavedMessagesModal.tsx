import { Bookmark, Hash, MessageSquare, X } from "lucide-react";
import type { SavedMessage } from "../types";
import { firstLines, formatTime } from "../ui-utils";

type SavedMessagesModalProps = {
  open: boolean;
  items: SavedMessage[];
  onOpenItem: (item: SavedMessage) => void;
  onUnsaveItem: (item: SavedMessage) => void;
  onClose: () => void;
};

export function SavedMessagesModal({
  open,
  items,
  onOpenItem,
  onUnsaveItem,
  onClose,
}: SavedMessagesModalProps) {
  if (!open) return null;

  return (
    <div className="search-backdrop" onClick={onClose}>
      <section className="inbox-panel saved-panel" onClick={(event) => event.stopPropagation()}>
        <header className="inbox-head">
          <button className="inbox-back" onClick={onClose} aria-label="Close saved messages">←</button>
          <div>
            <h2>Saved</h2>
            <p>{items.length} saved {items.length === 1 ? "message" : "messages"}</p>
          </div>
        </header>

        <div className="inbox-body">
          {items.length === 0 && (
            <div className="search-empty">
              <Bookmark size={34} />
              <h3>No saved messages</h3>
              <p>Use the message save action or right-click a message to track it here.</p>
            </div>
          )}

          {items.map((item) => {
            const Icon = item.thread_root_id ? MessageSquare : Hash;
            return (
              <article
                key={item.id}
                className="inbox-row saved-row"
                onClick={() => onOpenItem(item)}
              >
                <div className="inbox-row-main">
                  <div className="inbox-row-meta">
                    <span>#{item.channel_name}</span>
                    <time>{formatTime(item.message_created_at)}</time>
                    <em>{item.thread_root_id ? "thread" : "channel"}</em>
                  </div>
                  <h3>
                    <Icon size={18} />
                    {item.sender_name || "Saved message"}
                  </h3>
                  <p>{firstLines(item.body, 3) || "Empty message"}</p>
                  <small>Open source</small>
                </div>
                <button
                  className="inbox-check saved-unsave"
                  title="Unsave"
                  onClick={(event) => {
                    event.stopPropagation();
                    onUnsaveItem(item);
                  }}
                >
                  <X size={18} />
                </button>
              </article>
            );
          })}
        </div>
      </section>
    </div>
  );
}
