import { useEffect, useState } from "react";

export function useIsNarrowScreen() {
  const [isNarrow, setIsNarrow] = useState(() =>
    typeof window !== "undefined" && typeof window.matchMedia === "function"
      ? window.matchMedia("(max-width: 768px)").matches
      : false,
  );
  useEffect(() => {
    if (
      typeof window === "undefined" ||
      typeof window.matchMedia !== "function"
    ) {
      return undefined;
    }
    const mediaQuery = window.matchMedia("(max-width: 768px)");
    setIsNarrow(mediaQuery.matches);
    const handler = (e: MediaQueryListEvent) => setIsNarrow(e.matches);
    mediaQuery.addEventListener("change", handler);
    return () => mediaQuery.removeEventListener("change", handler);
  }, []);
  return isNarrow;
}
