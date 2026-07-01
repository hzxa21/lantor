import { forwardRef, type ReactNode, type TextareaHTMLAttributes } from "react";

type ComposerReferenceTextareaProps = Omit<TextareaHTMLAttributes<HTMLTextAreaElement>, "value"> & {
  value: string;
};

const REFERENCE_TOKEN_PATTERN = /\[\[(message|thread):([^\]\s]+)\]\]/gi;

function renderReferenceText(value: string) {
  const parts: ReactNode[] = [];
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  while ((match = REFERENCE_TOKEN_PATTERN.exec(value)) !== null) {
    if (match.index > lastIndex) {
      parts.push(value.slice(lastIndex, match.index));
    }
    const kind = match[1].toLowerCase();
    const id = match[2];
    parts.push(
      <span key={`${match.index}:${match[0]}`} className={`composer-reference-token ${kind}`}>
        {kind === "thread" ? "Thread" : "Message"} {id.slice(0, 8)}
      </span>,
    );
    lastIndex = match.index + match[0].length;
  }
  if (lastIndex < value.length) {
    parts.push(value.slice(lastIndex));
  }
  if (parts.length === 0) return value;
  return parts;
}

export const ComposerReferenceTextarea = forwardRef<HTMLTextAreaElement, ComposerReferenceTextareaProps>(
  function ComposerReferenceTextarea({ value, className, ...props }, ref) {
    const hasReferenceToken = REFERENCE_TOKEN_PATTERN.test(value);
    REFERENCE_TOKEN_PATTERN.lastIndex = 0;
    return (
      <div className={`composer-reference-input ${hasReferenceToken ? "has-reference-token" : ""}`}>
        <div className="composer-reference-overlay" aria-hidden="true">
          {value ? renderReferenceText(value) : "\u00a0"}
        </div>
        <textarea
          ref={ref}
          className={className}
          value={value}
          {...props}
        />
      </div>
    );
  },
);
