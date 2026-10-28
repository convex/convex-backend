import { useState, useLayoutEffect } from "react";

// Inspired by https://www.robinwieruch.de/react-custom-hook-check-if-overflow/.
export function useIsOverflowing(ref: React.RefObject<HTMLElement | null>) {
  const [isOverflow, setIsOverflow] = useState(false);

  // Force this overflow check to happen every time a component rerenders.
  const force = Math.random();

  useLayoutEffect(() => {
    const { current } = ref;

    if (current) {
      const hasOverflow = current.scrollWidth > current.clientWidth;
      setIsOverflow(hasOverflow);
    }
  }, [ref, force]);

  return isOverflow;
}
