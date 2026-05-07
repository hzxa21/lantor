import { BarChart3, Braces, Code2, FileImage, FileText, GitBranch, Table2, Workflow } from "lucide-react";
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
    case "chart":
      return BarChart3;
    case "diff":
      return GitBranch;
    case "mermaid":
      return Workflow;
    case "svg":
      return FileImage;
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

function staticHtmlPreviewDocument(content: string) {
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

function parseChartRows(content: string): Array<{ label: string; value: number }> {
  try {
    const parsed = JSON.parse(content);
    const rows: unknown[] = Array.isArray(parsed)
      ? parsed
      : Array.isArray(parsed.data)
        ? parsed.data
        : Array.isArray(parsed.labels) && Array.isArray(parsed.values)
          ? parsed.labels.map((label: unknown, index: number) => ({ label, value: parsed.values[index] }))
          : Array.isArray(parsed.labels) && Array.isArray(parsed.datasets?.[0]?.data)
            ? parsed.labels.map((label: unknown, index: number) => ({ label, value: parsed.datasets[0].data[index] }))
            : [];
    return rows
      .map((row: unknown, index: number) => {
        if (typeof row === "number") return { label: String(index + 1), value: row };
        if (!row || typeof row !== "object") return null;
        const record = row as Record<string, unknown>;
        const label = record.label ?? record.name ?? record.key ?? String(index + 1);
        const value = Number(record.value ?? record.count ?? record.y ?? record.total);
        return Number.isFinite(value) ? { label: String(label), value } : null;
      })
      .filter((row): row is { label: string; value: number } => Boolean(row))
      .slice(0, 16);
  } catch {
    return [];
  }
}

function ChartArtifact({ artifact }: { artifact: Artifact }) {
  const rows = parseChartRows(artifact.content);
  if (rows.length === 0) {
    return <pre>{artifact.content || previewContent(artifact)}</pre>;
  }
  const maxValue = Math.max(...rows.map((row) => Math.abs(row.value)), 1);
  return (
    <div className="artifact-chart-content">
      {rows.map((row, index) => (
        <div key={`${row.label}-${index}`} className="artifact-chart-row">
          <span>{row.label}</span>
          <div><i style={{ width: `${Math.max(4, (Math.abs(row.value) / maxValue) * 100)}%` }} /></div>
          <strong>{row.value.toLocaleString()}</strong>
        </div>
      ))}
    </div>
  );
}

function ArtifactContent({ artifact }: { artifact: Artifact }) {
  if (artifact.kind === "markdown") {
    return (
      <div className="artifact-markdown-content">
        <MessageMarkdown body={artifact.content || previewContent(artifact)} />
      </div>
    );
  }

  if (artifact.kind === "chart") {
    return <ChartArtifact artifact={artifact} />;
  }

  if (artifact.kind === "mermaid") {
    return <pre>{artifact.content || previewContent(artifact)}</pre>;
  }

  if (artifact.kind === "svg") {
    return (
      <div className="artifact-html-content">
        <iframe
          title={`SVG artifact preview: ${artifact.title}`}
          srcDoc={staticHtmlPreviewDocument(artifact.content || previewContent(artifact))}
          sandbox=""
        />
        <small>Sandboxed SVG preview. Scripts and network access are disabled.</small>
      </div>
    );
  }

  if (artifact.kind === "html") {
    return (
      <div className="artifact-html-content">
        <iframe
          title={`Artifact preview: ${artifact.title}`}
          srcDoc={staticHtmlPreviewDocument(artifact.content || previewContent(artifact))}
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
