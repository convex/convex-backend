import React from "react";
import { ReactNode } from "react";
import { useConvexAuth } from "./ConvexAuthState.js";

/**
 * Renders children if the client is authenticated.
 *
 * @public
 */
export function Authenticated({ children }: { children: ReactNode }) {
  const { isLoading, isAuthenticated } = useConvexAuth();
  if (isLoading || !isAuthenticated) {
    return null;
  }
  return <>{children}</>;
}

/**
 * Renders children if the client is using authentication but is not authenticated.
 *
 * @public
 */
export function Unauthenticated({ children }: { children: ReactNode }) {
  const { isLoading, isAuthenticated } = useConvexAuth();
  if (isLoading || isAuthenticated) {
    return null;
  }
  return <>{children}</>;
}

/**
 * Renders children if the client isn't using authentication or is in the process
 * of authenticating.
 *
 * @public
 */
export function AuthLoading({ children }: { children: ReactNode }) {
  const { isLoading } = useConvexAuth();
  if (!isLoading) {
    return null;
  }
  return <>{children}</>;
}

/**
 * Renders children while the client is refreshing the auth token for an
 * already-authenticated session (the server rejected the current token and
 * the socket is paused while a new one is fetched). Routine background
 * token rotation does not trigger this state.
 *
 * Whether used inside of `<Authenticated>` or not, children will only be
 * rendered if the user is authenticated.
 *
 * @public
 */
export function AuthRefreshing({ children }: { children: ReactNode }) {
  const { isAuthenticated, isRefreshing } = useConvexAuth();
  if (!isAuthenticated || !isRefreshing) {
    return null;
  }
  return <>{children}</>;
}
