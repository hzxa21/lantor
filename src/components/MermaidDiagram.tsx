import { type WheelEvent, useEffect, useId, useState } from "react";

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
const MIN_ZOOM = 0.5;
const MAX_ZOOM = 3;
const mermaidRenderCache = new Map<string, MermaidRenderState>();

function clampZoom(value: number) {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value));
}

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
  const [expanded, setExpanded] = useState(false);
  const [zoom, setZoom] = useState(1);

  function changeZoom(nextZoom: number, canvas?: HTMLElement | null, anchor?: { x: number; y: number }) {
    setZoom((currentZoom) => {
      const clamped = clampZoom(nextZoom);
      if (!canvas || !anchor || clamped === currentZoom) return clamped;
      const rect = canvas.getBoundingClientRect();
      const offsetX = anchor.x - rect.left;
      const offsetY = anchor.y - rect.top;
      const contentX = (canvas.scrollLeft + offsetX) / currentZoom;
      const contentY = (canvas.scrollTop + offsetY) / currentZoom;
      window.requestAnimationFrame(() => {
        canvas.scrollLeft = contentX * clamped - offsetX;
        canvas.scrollTop = contentY * clamped - offsetY;
      });
      return clamped;
    });
  }

  function handleLightboxWheel(event: WheelEvent<HTMLDivElement>) {
    if (!event.ctrlKey && !event.metaKey) return;
    event.preventDefault();
    const direction = event.deltaY > 0 ? -1 : 1;
    const factor = direction > 0 ? 1.08 : 1 / 1.08;
    changeZoom(zoom * factor, event.currentTarget, { x: event.clientX, y: event.clientY });
  }

  useEffect(() => {
    if (!expanded) {
      setZoom(1);
    }
  }, [expanded]);

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
      <>
        <figure className="mermaid-diagram">
          <button type="button" className="mermaid-expand" onClick={() => setExpanded(true)}>
            Expand
          </button>
          <div aria-label={title} dangerouslySetInnerHTML={{ __html: state.svg }} />
        </figure>
        {expanded && (
          <div className="mermaid-lightbox" role="dialog" aria-modal="true" aria-label={title}>
            <div className="mermaid-lightbox-card">
              <header>
                <strong>{title}</strong>
                <div className="mermaid-lightbox-actions">
                  <button type="button" onClick={() => changeZoom(zoom - 0.25)}>-</button>
                  <span>{Math.round(zoom * 100)}%</span>
                  <button type="button" onClick={() => changeZoom(zoom + 0.25)}>+</button>
                  <button type="button" onClick={() => changeZoom(1)}>Reset</button>
                  <button type="button" onClick={() => setExpanded(false)}>Close</button>
                </div>
              </header>
              <div className="mermaid-lightbox-canvas" onWheel={handleLightboxWheel}>
                <div
                  className="mermaid-lightbox-zoom"
                  style={{ transform: `scale(${zoom})` }}
                  dangerouslySetInnerHTML={{ __html: state.svg }}
                />
              </div>
              <details>
                <summary>Source</summary>
                <pre>{normalizedSource}</pre>
              </details>
            </div>
          </div>
        )}
      </>
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
