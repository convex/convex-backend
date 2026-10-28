import { useEffect, useState } from "react";

const MOBILE_DEVICE_QUERY = "(pointer: coarse) and (hover: none)";

export function useIsMobileDevice() {
  const [isMobile, setIsMobile] = useState(
    () =>
      typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia(MOBILE_DEVICE_QUERY).matches,
  );
  useEffect(() => {
    if (
      typeof window === "undefined" ||
      typeof window.matchMedia !== "function"
    ) {
      return undefined;
    }
    const mediaQuery = window.matchMedia(MOBILE_DEVICE_QUERY);
    setIsMobile(mediaQuery.matches);
    const handler = (e: MediaQueryListEvent) => setIsMobile(e.matches);
    mediaQuery.addEventListener("change", handler);
    return () => mediaQuery.removeEventListener("change", handler);
  }, []);
  return isMobile;
}
