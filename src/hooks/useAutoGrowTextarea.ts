import { useCallback, useLayoutEffect, useRef, type RefObject } from "react";

function cssPixelValue(value: string) {
  const parsed = Number.parseFloat(value);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : Number.POSITIVE_INFINITY;
}

export function useAutoGrowTextarea(textareaRef: RefObject<HTMLTextAreaElement | null>, value: string) {
  const animationFrameRef = useRef<number | null>(null);
  const lastAppliedHeightRef = useRef<number | null>(null);
  const lastObservedWidthRef = useRef<number | null>(null);
  const isFocusedRef = useRef(false);
  const isComposingRef = useRef(false);

  const resizeTextarea = useCallback((allowShrink = true) => {
    const textareaElement = textareaRef.current;
    if (!textareaElement) return;
    const computedStyle = window.getComputedStyle(textareaElement);
    const maxHeight = cssPixelValue(computedStyle.maxHeight);

    if (allowShrink) {
      textareaElement.style.height = "auto";
      const nextHeight = Math.min(textareaElement.scrollHeight, maxHeight);
      textareaElement.style.height = `${nextHeight}px`;
      lastAppliedHeightRef.current = nextHeight;
      textareaElement.style.overflowY = textareaElement.scrollHeight > maxHeight ? "auto" : "hidden";
      return;
    }

    const currentHeight = lastAppliedHeightRef.current ?? textareaElement.clientHeight;
    const nextHeight = Math.min(textareaElement.scrollHeight, maxHeight);
    if (nextHeight > currentHeight + 0.5) {
      textareaElement.style.height = `${nextHeight}px`;
      lastAppliedHeightRef.current = nextHeight;
    }
    textareaElement.style.overflowY = textareaElement.scrollHeight > maxHeight ? "auto" : "hidden";
  }, [textareaRef]);

  const scheduleResize = useCallback((allowShrink = true) => {
    if (animationFrameRef.current !== null) window.cancelAnimationFrame(animationFrameRef.current);
    animationFrameRef.current = window.requestAnimationFrame(() => {
      animationFrameRef.current = null;
      if (isComposingRef.current) return;
      resizeTextarea(allowShrink);
    });
  }, [resizeTextarea]);

  useLayoutEffect(() => {
    if (isComposingRef.current) return;
    resizeTextarea(!isFocusedRef.current || value.length === 0);
  }, [resizeTextarea, value]);

  useLayoutEffect(() => {
    const textareaElement = textareaRef.current;
    if (!textareaElement) return;
    const textarea: HTMLTextAreaElement = textareaElement;
    lastObservedWidthRef.current = textarea.clientWidth;
    const handleFocus = () => {
      isFocusedRef.current = true;
    };
    const handleBlur = () => {
      isFocusedRef.current = false;
      isComposingRef.current = false;
      scheduleResize();
    };
    const handleCompositionStart = () => {
      isComposingRef.current = true;
    };
    const handleCompositionEnd = () => {
      isComposingRef.current = false;
      scheduleResize(!isFocusedRef.current);
    };
    const handleWindowResize = () => {
      scheduleResize(!isFocusedRef.current);
    };

    const observer = typeof ResizeObserver === "undefined"
      ? null
      : new ResizeObserver((entries) => {
        const observedWidth = entries[0]?.contentRect.width ?? textarea.clientWidth;
        if (lastObservedWidthRef.current !== null && Math.abs(observedWidth - lastObservedWidthRef.current) < 0.5) {
          return;
        }
        lastObservedWidthRef.current = observedWidth;
        scheduleResize(!isFocusedRef.current);
      });
    observer?.observe(textarea);
    textarea.addEventListener("focus", handleFocus);
    textarea.addEventListener("blur", handleBlur);
    textarea.addEventListener("compositionstart", handleCompositionStart);
    textarea.addEventListener("compositionend", handleCompositionEnd);
    window.addEventListener("resize", handleWindowResize);

    return () => {
      if (animationFrameRef.current !== null) window.cancelAnimationFrame(animationFrameRef.current);
      animationFrameRef.current = null;
      observer?.disconnect();
      textarea.removeEventListener("focus", handleFocus);
      textarea.removeEventListener("blur", handleBlur);
      textarea.removeEventListener("compositionstart", handleCompositionStart);
      textarea.removeEventListener("compositionend", handleCompositionEnd);
      window.removeEventListener("resize", handleWindowResize);
    };
  }, [scheduleResize, textareaRef]);
}
