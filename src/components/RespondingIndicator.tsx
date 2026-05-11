type RespondingIndicatorProps = {
  names: string[];
};

function respondingLabel(names: string[]) {
  if (names.length === 0) return "";
  if (names.length === 1) return `${names[0]} is responding`;
  if (names.length === 2) return `${names[0]} and ${names[1]} are responding`;
  return `${names[0]} and ${names.length - 1} others are responding`;
}

export function RespondingIndicator({ names }: RespondingIndicatorProps) {
  if (names.length === 0) return null;
  return (
    <div className="responding-indicator" role="status" aria-live="polite">
      <span>{respondingLabel(names)}</span>
      <i aria-hidden="true"><b /><b /><b /></i>
    </div>
  );
}
