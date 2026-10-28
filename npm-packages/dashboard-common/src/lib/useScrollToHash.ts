import { useEffect, type RefObject } from "react";

export function useScrollToHash<T extends HTMLElement>(
  hash: string,
  targetRef: RefObject<T | null>,
) {
  useEffect(() => {
    if (typeof window === "undefined") {
      return;
    }

    if (window.location.hash === hash) {
      targetRef.current?.scrollIntoView({
        behavior: "smooth",
        block: "start",
      });
    }
  }, [hash, targetRef]);
}
