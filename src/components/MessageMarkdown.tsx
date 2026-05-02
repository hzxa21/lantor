import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";

type MessageMarkdownProps = {
  body: string;
};

export function MessageMarkdown({ body }: MessageMarkdownProps) {
  return (
    <div className="markdown-body">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          a: ({ children, ...props }) => (
            <a {...props} target="_blank" rel="noreferrer">
              {children}
            </a>
          ),
        }}
      >
        {body}
      </ReactMarkdown>
    </div>
  );
}
