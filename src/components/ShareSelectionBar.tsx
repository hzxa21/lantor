import { Copy, Download, X } from "lucide-react";

type ShareSelectionBarProps = {
  count: number;
  total: number;
  onSelectAll: () => void;
  onCancel: () => void;
  onCopyMarkdown: () => void;
  onDownloadImage: () => void;
};

export function ShareSelectionBar({
  count,
  total,
  onSelectAll,
  onCancel,
  onCopyMarkdown,
  onDownloadImage,
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
        <button type="button" className="accent" disabled={count === 0} onClick={onDownloadImage}>
          <Download size={16} />
          Download image
        </button>
        <button type="button" disabled={count === 0} onClick={onCopyMarkdown}>
          <Copy size={16} />
          Copy MD
        </button>
      </div>
    </div>
  );
}
