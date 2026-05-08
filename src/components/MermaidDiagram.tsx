import { useEffect, useId, useState } from "react";

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
const mermaidRenderCache = new Map<string, MermaidRenderState>();

export function looksLikeMermaid(value: string) {
  const source = value.trim();
  if (!source || source.length > MERMAID_SOURCE_LIMIT) return false;
  if (!MERMAID_START.test(source)) return false;
  return source.includes("\n") || /^(flowchart|graph)\s+\S+/i.test(source);
}

export function MermaidDiagram({ source, title = "Mermaid diagram" }: MermaidDiagramProps) {
  const reactId = useId().replace(/[^A-Za-z0-9_-]/g, "");
  const normalizedSource = source.trim();
  const cacheKey = normalizedSource;
  const [state, setState] = useState<MermaidRenderState>(() => (
    mermaidRenderCache.get(cacheKey) ?? { status: "loading" }
  ));

  useEffect(() => {
    let cancelled = false;
    if (!normalizedSource || normalizedSource.length > MERMAID_SOURCE_LIMIT) {
      const errorState = { status: "error", message: "Mermaid source is empty or too large to render inline." } as const;
      mermaidRenderCache.set(cacheKey, errorState);
      setState(errorState);
      return;
    }
    const cached = mermaidRenderCache.get(cacheKey);
    if (cached) {
      setState(cached);
      return;
    }
    setState((current) => current.status === "ready" ? current : { status: "loading" });
    import("mermaid")
      .then(({ default: mermaid }) => {
        mermaid.initialize({
          startOnLoad: false,
          securityLevel: "strict",
          theme: "default",
          fontFamily: "ui-sans-serif, system-ui, sans-serif",
        });
        return mermaid.render(`localslock-mermaid-${reactId}`, normalizedSource);
      })
      .then(({ svg }) => {
        const readyState = { status: "ready", svg } as const;
        mermaidRenderCache.set(cacheKey, readyState);
        if (!cancelled) setState(readyState);
      })
      .catch((err) => {
        const message = err instanceof Error ? err.message : String(err);
        const errorState = { status: "error", message } as const;
        mermaidRenderCache.set(cacheKey, errorState);
        if (!cancelled) setState(errorState);
      });
    return () => {
      cancelled = true;
    };
  }, [cacheKey, normalizedSource, reactId]);

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
