import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { MermaidDiagram, looksLikeMermaid } from "./MermaidDiagram";

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

export function MessageMarkdown({ body }: MessageMarkdownProps) {
  if (looksLikeMermaid(body)) {
    return (
      <div className="markdown-body">
        <MermaidDiagram source={body} />
      </div>
    );
  }

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
          code: ({ children, className, ...props }) => {
            const content = String(children).replace(/\n$/, "");
            const language = /language-([A-Za-z0-9_-]+)/.exec(className || "")?.[1]?.toLowerCase();
            if (language === "mermaid" || looksLikeMermaid(content)) {
              return <MermaidDiagram source={content} />;
            }
            return <code {...props} className={className}>{children}</code>;
          },
        }}
      >
        {linkedBody}
      </ReactMarkdown>
    </div>
  );
}
