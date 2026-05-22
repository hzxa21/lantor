import { useEffect, useState } from "react";

const MOBILE_VIEWPORT_QUERY = "(max-width: 760px)";

function matchesMobileViewport() {
  return typeof window !== "undefined" && window.matchMedia(MOBILE_VIEWPORT_QUERY).matches;
}

export function useMobileViewport() {
  const [isMobileViewport, setIsMobileViewport] = useState(matchesMobileViewport);

  useEffect(() => {
    const mediaQuery = window.matchMedia(MOBILE_VIEWPORT_QUERY);
    function updateViewportMatch() {
      setIsMobileViewport(mediaQuery.matches);
    }

    updateViewportMatch();
    mediaQuery.addEventListener("change", updateViewportMatch);
    return () => mediaQuery.removeEventListener("change", updateViewportMatch);
  }, []);

  return isMobileViewport;
}
