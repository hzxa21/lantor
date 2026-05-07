import { Braces, Code2, FileText, GitBranch, Table2, Workflow } from "lucide-react";
import { Artifact } from "../types";
import { MessageMarkdown } from "./MessageMarkdown";

type MessageArtifactsProps = {
  artifacts: Artifact[];
  onOpenArtifact?: (artifact: Artifact) => void;
};

function artifactIcon(kind: string) {
  switch (kind) {
    case "json":
      return Braces;
    case "table":
      return Table2;
    case "diff":
      return GitBranch;
    case "mermaid":
      return Workflow;
    case "html":
    case "text":
      return Code2;
    default:
      return FileText;
  }
}

function previewContent(artifact: Artifact) {
  const content = artifact.summary || artifact.content;
  const compact = content.trim().replace(/\s+/g, " ");
  return compact.length > 140 ? `${compact.slice(0, 140)}...` : compact;
}

function htmlPreviewDocument(content: string) {
  const policy = [
    "default-src 'none'",
    "img-src data: blob:",
    "style-src 'unsafe-inline'",
    "font-src data:",
  ].join("; ");
  const headPrefix = `<meta http-equiv="Content-Security-Policy" content="${policy}"><base target="_blank">`;
  if (/<head[\s>]/i.test(content)) {
    return content.replace(/<head([^>]*)>/i, `<head$1>${headPrefix}`);
  }
  return `<!doctype html><html><head>${headPrefix}</head><body>${content}</body></html>`;
}

function ArtifactContent({ artifact }: { artifact: Artifact }) {
  if (artifact.kind === "markdown") {
    return (
      <div className="artifact-markdown-content">
        <MessageMarkdown body={artifact.content || previewContent(artifact)} />
      </div>
    );
  }

  if (artifact.kind === "html") {
    return (
      <div className="artifact-html-content">
        <iframe
          title={`Artifact preview: ${artifact.title}`}
          srcDoc={htmlPreviewDocument(artifact.content || previewContent(artifact))}
          sandbox=""
        />
        <small>Sandboxed HTML preview. Scripts and same-origin access are disabled.</small>
      </div>
    );
  }

  return <pre>{artifact.content || previewContent(artifact)}</pre>;
}

export function MessageArtifacts({ artifacts, onOpenArtifact }: MessageArtifactsProps) {
  if (artifacts.length === 0) return null;

  return (
    <div className="message-artifacts">
      {artifacts.map((artifact) => {
        const Icon = artifactIcon(artifact.kind);
        return (
          <details key={artifact.id} className={`message-artifact artifact-${artifact.kind}`}>
            <summary>
              <span className="artifact-icon"><Icon size={16} /></span>
              <span>
                <strong>{artifact.title}</strong>
                <small>{artifact.kind} · artifact {artifact.id.slice(0, 8)}</small>
              </span>
            </summary>
            {onOpenArtifact && (
              <div className="artifact-actions">
                <button type="button" onClick={() => onOpenArtifact(artifact)}>Open full artifact</button>
              </div>
            )}
            {artifact.summary && <p>{artifact.summary}</p>}
            <ArtifactContent artifact={artifact} />
          </details>
        );
      })}
    </div>
  );
}
