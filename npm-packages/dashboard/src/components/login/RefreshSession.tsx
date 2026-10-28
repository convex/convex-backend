import { useWorkOS } from "hooks/useWorkOS";
import { useAccessToken } from "hooks/useServerSideData";
import { useCallback, useEffect, useRef } from "react";

// The auth session is stored in a cookie, and if we don't call the Next.js server
// WorkOS ends the session after a few days of inactivity. It also always ends sessions
// after a fixed duration after the session creation, regardless of activity
// (the exact limits are set in the WorkOS dashboard, under Applications > [application] > Sessions).
// By calling the server periodically we get an updated cookie via the Set-Cookie header.
// We also need to refresh the auth token (which expires after 24 hours).
export function RefreshSession() {
  const { user } = useWorkOS();
  const lastRefreshed = useRef<number | undefined>(undefined);
  const [_, setAuthToken] = useAccessToken();

  const refresh = useCallback(
    async (forceRefresh: boolean) => {
      try {
        if (
          forceRefresh ||
          (lastRefreshed.current !== undefined &&
            lastRefreshed.current < Date.now() - REFRESH_INTERVAL_IN_MS)
        ) {
          const response = await fetch("/api/auth/refresh", { method: "POST" });
          const body = (await response.json()) as
            | { error: string }
            | { accessToken: string };
          if ("accessToken" in body) {
            setAuthToken(body.accessToken);
          }
          lastRefreshed.current = Date.now();
        }
      } catch {
        // we don't care if this request fails
      }
    },
    [setAuthToken],
  );

  useEffect(() => {
    let interval: number | undefined;
    if (user) {
      interval = window.setInterval(async () => {
        await refresh(true);
      }, REFRESH_INTERVAL_IN_MS);
    }
    return () => {
      window.clearInterval(interval);
    };
  }, [user, refresh]);

  // Refresh immediately after browser becomes active
  // in case we were inactive for a long time
  useEffect(() => {
    const onTabActive = async () => {
      await refresh(false);
    };
    window.addEventListener("focus", onTabActive);
    return () => {
      window.removeEventListener("focus", onTabActive);
    };
  }, [refresh]);

  return null;
}

const TEN_MINUTES_IN_MS = 10 * 60 * 1000;
const REFRESH_INTERVAL_IN_MS = TEN_MINUTES_IN_MS;
