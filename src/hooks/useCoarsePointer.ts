import { useEffect, useState } from "react";

const COARSE_POINTER_QUERY = "(hover: none) and (pointer: coarse)";

function matchesCoarsePointer() {
  return typeof window !== "undefined" && window.matchMedia(COARSE_POINTER_QUERY).matches;
}

export function useCoarsePointer() {
  const [isCoarsePointer, setIsCoarsePointer] = useState(matchesCoarsePointer);

  useEffect(() => {
    const mediaQuery = window.matchMedia(COARSE_POINTER_QUERY);
    function updatePointerMatch() {
      setIsCoarsePointer(mediaQuery.matches);
    }

    updatePointerMatch();
    mediaQuery.addEventListener("change", updatePointerMatch);
    return () => mediaQuery.removeEventListener("change", updatePointerMatch);
  }, []);

  return isCoarsePointer;
}
