export type RespondingIndicatorItem = {
  name: string;
  state: string;
};

type RespondingIndicatorProps = {
  items: RespondingIndicatorItem[];
};

function itemLabel(item: RespondingIndicatorItem) {
  return `${item.name} · ${item.state}`;
}

function respondingLabel(items: RespondingIndicatorItem[]) {
  if (items.length === 0) return "";
  if (items.length === 1) return itemLabel(items[0]);
  if (items.length === 2) return `${itemLabel(items[0])}, ${itemLabel(items[1])}`;
  return `${itemLabel(items[0])} and ${items.length - 1} others`;
}

export function RespondingIndicator({ items }: RespondingIndicatorProps) {
  if (items.length === 0) return null;
  return (
    <div className="responding-indicator" role="status" aria-live="polite">
      <span>{respondingLabel(items)}</span>
      <i aria-hidden="true"><b /><b /><b /></i>
    </div>
  );
}
