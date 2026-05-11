import { Copy, Download, X } from "lucide-react";

type ShareSelectionBarProps = {
  count: number;
  total: number;
  downloadFileName?: string;
  downloadPending?: boolean;
  downloadUrl: string | null;
  onSelectAll: () => void;
  onCancel: () => void;
  onCopyMarkdown: () => void;
};

export function ShareSelectionBar({
  count,
  total,
  downloadFileName = "localslock-share.png",
  downloadPending = false,
  downloadUrl,
  onSelectAll,
  onCancel,
  onCopyMarkdown,
}: ShareSelectionBarProps) {
  return (
    <div className="share-selection-bar">
      <strong>{count} selected</strong>
      <div>
        <button type="button" disabled={count === total} onClick={onSelectAll}>
          Select all
        </button>
        <button type="button" onClick={onCancel}>
          <X size={16} />
          Cancel
        </button>
        {count > 0 && downloadUrl ? (
          <a className="accent" href={downloadUrl} download={downloadFileName}>
            <Download size={16} />
            Download image
          </a>
        ) : (
          <button type="button" className="accent" disabled>
            <Download size={16} />
            {downloadPending ? "Preparing image" : "Download image"}
          </button>
        )}
        <button type="button" disabled={count === 0} onClick={onCopyMarkdown}>
          <Copy size={16} />
          Copy MD
        </button>
      </div>
    </div>
  );
}
