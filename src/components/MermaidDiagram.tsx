import { useEffect, useRef, useState } from "react";

type MermaidDiagramProps = {
  source: string;
  title?: string;
};

type MermaidRenderState =
  | { status: "loading" }
  | { status: "ready"; svg: string }
  | { status: "error"; message: string };

const MERMAID_START = /^(flowchart|graph|sequenceDiagram|classDiagram|stateDiagram(?:-v2)?|erDiagram|journey|gantt|pie|quadrantChart|requirementDiagram|gitGraph|mindmap|timeline|sankey-beta|xychart-beta|block-beta|packet-beta|architecture-beta)\b/i;
const MERMAID_SOURCE_LIMIT = 80_000;

export function looksLikeMermaid(value: string) {
  const source = value.trim();
  if (!source || source.length > MERMAID_SOURCE_LIMIT) return false;
  if (!MERMAID_START.test(source)) return false;
  return source.includes("\n") || /^(flowchart|graph)\s+\S+/i.test(source);
}

export function MermaidDiagram({ source, title = "Mermaid diagram" }: MermaidDiagramProps) {
  const renderIdRef = useRef(`localslock-mermaid-${Math.random().toString(36).slice(2)}`);
  const [state, setState] = useState<MermaidRenderState>({ status: "loading" });
  const normalizedSource = source.trim();

  useEffect(() => {
    let cancelled = false;
    if (!normalizedSource || normalizedSource.length > MERMAID_SOURCE_LIMIT) {
      setState({ status: "error", message: "Mermaid source is empty or too large to render inline." });
      return;
    }
    setState({ status: "loading" });
    import("mermaid")
      .then(({ default: mermaid }) => {
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: "default",
          fontFamily: "ui-sans-serif, system-ui, sans-serif",
        });
        return mermaid.render(renderIdRef.current, normalizedSource);
      })
      .then(({ svg }) => {
        if (!cancelled) setState({ status: "ready", svg });
      })
      .catch((err) => {
        const message = err instanceof Error ? err.message : String(err);
        if (!cancelled) setState({ status: "error", message });
      });
    return () => {
      cancelled = true;
    };
  }, [normalizedSource]);

  if (state.status === "ready") {
    return (
      <figure className="mermaid-diagram">
        <div aria-label={title} dangerouslySetInnerHTML={{ __html: state.svg }} />
      </figure>
    );
  }

  if (state.status === "error") {
    return (
      <div className="mermaid-fallback">
        <strong>Mermaid render failed</strong>
        <small>{state.message}</small>
        <pre>{normalizedSource}</pre>
      </div>
    );
  }

  return (
    <div className="mermaid-loading">
      Rendering diagram...
    </div>
  );
}
