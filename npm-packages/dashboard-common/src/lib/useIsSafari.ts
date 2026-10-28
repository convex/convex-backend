import { useEffect, useState } from "react";

/**
 * Detect Safari (desktop or iOS) from the user agent.
 * SSR-safe: returns `false` until mounted on the client.
 */
export function useIsSafari(): boolean {
  const [isSafari, setIsSafari] = useState(false);
  useEffect(() => {
    setIsSafari(
      // https://stackoverflow.com/a/23522755
      /^((?!chrome|android).)*safari/i.test(navigator.userAgent),
    );
  }, []);
  return isSafari;
}
