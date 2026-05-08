import { Children, ReactNode, isValidElement, useState } from "react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

type MessageMarkdownProps = {
  body: string;
};

const INLINE_CODE_SPLIT = /(`[^`\n]*(?:`|$))/g;
const FENCE_SPLIT = /(```[\s\S]*?(?:```|$))/g;

function encodeLocalPath(value: string) {
  return encodeURIComponent(value.replace(/^[@#]/, ""));
}

function linkifyPlainText(value: string) {
  return value
    .replace(/(^|[\s([{])@([A-Za-z][A-Za-z0-9_-]{1,31})(?=$|[\s.,;:!?)\]}])/g, (_match, prefix, handle) => (
      `${prefix}[@${handle}](/localslock/agent/${encodeLocalPath(handle)})`
    ))
    .replace(/(^|[\s([{])#([A-Za-z][A-Za-z0-9_-]{1,63})(?=$|[\s.,;:!?)\]}])/g, (_match, prefix, channel) => (
      `${prefix}[#${channel}](/localslock/channel/${encodeLocalPath(channel)})`
    ))
    .replace(/(^|[\s([{])task #([0-9]+)(?=$|[\s.,;:!?)\]}])/gi, (_match, prefix, taskNumber) => (
      `${prefix}[task #${taskNumber}](/localslock/task/${taskNumber})`
    ));
}

function linkifyMessageBody(body: string) {
  return body
    .split(FENCE_SPLIT)
    .map((segment) => {
      if (segment.startsWith("```")) return segment;
      return segment
        .split(INLINE_CODE_SPLIT)
        .map((inlineSegment) => inlineSegment.startsWith("`") ? inlineSegment : linkifyPlainText(inlineSegment))
        .join("");
    })
    .join("");
}

function textFromNode(node: ReactNode): string {
  if (typeof node === "string" || typeof node === "number") return String(node);
  if (Array.isArray(node)) return node.map(textFromNode).join("");
  if (isValidElement<{ children?: ReactNode }>(node)) return textFromNode(node.props.children);
  return "";
}

async function copyText(value: string) {
  if (navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return;
    } catch {
      // Fall through to the DOM fallback for WebView permission edge cases.
    }
  }
  const textarea = document.createElement("textarea");
  textarea.value = value;
  textarea.setAttribute("readonly", "true");
  textarea.style.position = "fixed";
  textarea.style.opacity = "0";
  document.body.appendChild(textarea);
  textarea.select();
  document.execCommand("copy");
  textarea.remove();
}

function CopyableCodeBlock({ children }: { children?: ReactNode }) {
  const [copied, setCopied] = useState(false);
  const text = textFromNode(children).replace(/\n$/, "");

  async function handleCopy() {
    await copyText(text);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1200);
  }

  return (
    <div className="code-block-shell">
      <button type="button" onClick={handleCopy} aria-label="Copy code block">
        {copied ? "Copied" : "Copy"}
      </button>
      <pre>{children}</pre>
    </div>
  );
}

export function MessageMarkdown({ body }: MessageMarkdownProps) {
  const linkedBody = linkifyMessageBody(body);
  return (
    <div className="markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ children, href, ...props }) => {
            const isLocalLink = href?.startsWith("/localslock/");
            return (
              <a
                {...props}
                href={href}
                className={isLocalLink ? "local-entity-link" : undefined}
                target={isLocalLink ? undefined : "_blank"}
                rel={isLocalLink ? undefined : "noreferrer"}
                onClick={isLocalLink ? (event) => event.preventDefault() : undefined}
              >
                {children}
              </a>
            );
          },
          pre: ({ children }) => (
            <CopyableCodeBlock>{Children.toArray(children)}</CopyableCodeBlock>
          ),
        }}
      >
        {linkedBody}
      </ReactMarkdown>
    </div>
  );
}
