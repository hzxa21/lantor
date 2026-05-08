import { memo, useEffect, useId, useRef, useState } from "react";
import { createPortal } from "react-dom";

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

const MermaidSvg = memo(function MermaidSvg({ svg, title }: { svg: string; title: string }) {
  return <div aria-label={title} dangerouslySetInnerHTML={{ __html: svg }} />;
});

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
  const [zoomLabel, setZoomLabel] = useState(100);
  const zoomRef = useRef(1);
  const zoomNodeRef = useRef<HTMLDivElement | null>(null);
  const zoomLabelFrameRef = useRef<number | null>(null);

  function changeZoom(nextZoom: number) {
    const currentZoom = zoomRef.current;
    const clamped = clampZoom(nextZoom);
    if (clamped === currentZoom) return;
    zoomRef.current = clamped;
    if (zoomNodeRef.current) {
      zoomNodeRef.current.style.zoom = String(clamped);
    }
    if (zoomLabelFrameRef.current === null) {
      zoomLabelFrameRef.current = window.requestAnimationFrame(() => {
        zoomLabelFrameRef.current = null;
        setZoomLabel(Math.round(zoomRef.current * 100));
      });
    }
  }

  useEffect(() => {
    if (!expanded) {
      zoomRef.current = 1;
      setZoomLabel(100);
      if (zoomNodeRef.current) {
        zoomNodeRef.current.style.zoom = "1";
      }
    }
  }, [expanded]);

  useEffect(() => () => {
    if (zoomLabelFrameRef.current !== null) {
      window.cancelAnimationFrame(zoomLabelFrameRef.current);
    }
  }, []);

  useEffect(() => {
    if (!expanded) return;
    function closeOnEscape(event: KeyboardEvent) {
      if (event.key === "Escape") setExpanded(false);
    }
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
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
          <button
            type="button"
            className="mermaid-expand"
            onClick={(event) => {
              event.stopPropagation();
              setExpanded(true);
            }}
          >
            Expand
          </button>
          <MermaidSvg svg={state.svg} title={title} />
        </figure>
        {expanded && createPortal(
          <div
            className="mermaid-lightbox"
            role="dialog"
            aria-modal="true"
            aria-label={title}
            onClick={(event) => event.stopPropagation()}
          >
            <div className="mermaid-lightbox-card">
              <header>
                <strong>{title}</strong>
                <div className="mermaid-lightbox-actions">
                  <button type="button" onClick={() => changeZoom(zoomRef.current - 0.25)}>-</button>
                  <span>{zoomLabel}%</span>
                  <button type="button" onClick={() => changeZoom(zoomRef.current + 0.25)}>+</button>
                  <button type="button" onClick={() => changeZoom(1)}>Reset</button>
                  <button type="button" onClick={() => setExpanded(false)}>Close</button>
                </div>
              </header>
              <div className="mermaid-lightbox-canvas">
                <div
                  ref={zoomNodeRef}
                  className="mermaid-lightbox-zoom"
                >
                  <MermaidSvg svg={state.svg} title={title} />
                </div>
              </div>
              <details>
                <summary>Source</summary>
                <pre>{normalizedSource}</pre>
              </details>
            </div>
          </div>,
          document.body,
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
