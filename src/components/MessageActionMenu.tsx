import { Bookmark, Copy, Link, MessageSquare, Quote } from "lucide-react";
import { useEffect, useRef } from "react";

type MessageActionMenuProps = {
  x: number;
  y: number;
  isSaved: boolean;
  onCopyLink: () => void;
  onCopyMarkdown: () => void;
  onCopyReferenceMessage?: () => void;
  onCopyReferenceThread?: () => void;
  onReferenceMessage?: () => void;
  onReferenceThread?: () => void;
  onToggleSaved: () => void;
  onClose: () => void;
};

export function MessageActionMenu({
  x,
  y,
  isSaved,
  onCopyLink,
  onCopyMarkdown,
  onCopyReferenceMessage,
  onCopyReferenceThread,
  onReferenceMessage,
  onReferenceThread,
  onToggleSaved,
  onClose,
}: MessageActionMenuProps) {
  const openedAtRef = useRef(Date.now());

  useEffect(() => {
    function handleClose() {
      if (Date.now() - openedAtRef.current < 350) return;
      onClose();
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.key === "Escape") onClose();
    }

    window.addEventListener("click", handleClose);
    window.addEventListener("scroll", handleClose, true);
    window.addEventListener("keydown", handleKeyDown);
    return () => {
      window.removeEventListener("click", handleClose);
      window.removeEventListener("scroll", handleClose, true);
      window.removeEventListener("keydown", handleKeyDown);
    };
  }, [onClose]);

  return (
    <div
      className="message-action-menu"
      style={{
        left: Math.max(12, Math.min(x, window.innerWidth - 248)),
        top: Math.max(12, Math.min(y, window.innerHeight - 320)),
      }}
      onPointerDown={(event) => event.stopPropagation()}
      onClick={(event) => event.stopPropagation()}
      onContextMenu={(event) => event.stopPropagation()}
      role="menu"
    >
      <button type="button" onClick={onCopyLink}>
        <Link size={18} />
        <span>Copy link</span>
      </button>
      <button type="button" onClick={onCopyMarkdown}>
        <Copy size={18} />
        <span>Copy markdown</span>
      </button>
      {onCopyReferenceMessage && (
        <button type="button" onClick={onCopyReferenceMessage}>
          <Copy size={18} />
          <span>Copy message reference</span>
        </button>
      )}
      {onCopyReferenceThread && (
        <button type="button" onClick={onCopyReferenceThread}>
          <Copy size={18} />
          <span>Copy thread reference</span>
        </button>
      )}
      {onReferenceMessage && (
        <button type="button" onClick={onReferenceMessage}>
          <Quote size={18} />
          <span>Reference message</span>
        </button>
      )}
      {onReferenceThread && (
        <button type="button" onClick={onReferenceThread}>
          <MessageSquare size={18} />
          <span>Reference thread</span>
        </button>
      )}
      <button type="button" onClick={onToggleSaved}>
        <Bookmark size={18} />
        <span>{isSaved ? "Unsave message" : "Save message"}</span>
      </button>
    </div>
  );
}
