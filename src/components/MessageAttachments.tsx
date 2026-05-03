import { convertFileSrc } from "@tauri-apps/api/core";
import { FileText, Image as ImageIcon } from "lucide-react";
import { MessageAttachment } from "../types";

type MessageAttachmentsProps = {
  attachments: MessageAttachment[];
};

function formatBytes(value: number) {
  if (value < 1024) return `${value} B`;
  if (value < 1024 * 1024) return `${(value / 1024).toFixed(1)} KB`;
  return `${(value / 1024 / 1024).toFixed(1)} MB`;
}

export function MessageAttachments({ attachments }: MessageAttachmentsProps) {
  if (attachments.length === 0) return null;

  return (
    <div className="message-attachments">
      {attachments.map((attachment) => {
        const src = convertFileSrc(attachment.storage_path);
        const isImage = attachment.mime_type.startsWith("image/");
        return (
          <a
            key={attachment.id}
            className={`message-attachment ${isImage ? "image" : ""}`}
            href={src}
            target="_blank"
            rel="noreferrer"
            title={attachment.original_name}
          >
            {isImage ? (
              <img src={src} alt={attachment.original_name} loading="lazy" />
            ) : (
              <span className="attachment-icon"><FileText size={18} /></span>
            )}
            <span className="attachment-meta">
              <span>{isImage ? <ImageIcon size={13} /> : <FileText size={13} />} {attachment.original_name}</span>
              <small>{attachment.mime_type || "file"} · {formatBytes(attachment.size_bytes)}</small>
            </span>
          </a>
        );
      })}
    </div>
  );
}
