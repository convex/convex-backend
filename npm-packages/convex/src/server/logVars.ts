const REQUEST_ID = Symbol("var.requestId");
const IP = Symbol("var.ip");
const USER_AGENT = Symbol("var.userAgent");
const NOW = Symbol("var.now");
const CONVEX_ACTOR = Symbol("var.convexActor");

export type LogVar =
  | typeof REQUEST_ID
  | typeof IP
  | typeof USER_AGENT
  | typeof NOW
  | typeof CONVEX_ACTOR;

export const varNames: Record<symbol, string> = {
  [REQUEST_ID]: "requestId",
  [IP]: "ip",
  [USER_AGENT]: "userAgent",
  [NOW]: "now",
  [CONVEX_ACTOR]: "convexActor",
};

export const vars = {
  /** Resolved to the request ID. */
  requestId: REQUEST_ID,
  /** Resolved to the client's IP address. */
  ip: IP,
  /** Resolved to the client's User-Agent header. */
  userAgent: USER_AGENT,
  /**
   * Resolved to the current server timestamp, as milliseconds from the
   * Unix epoch.
   */
  now: NOW,
  /**
   * If the function was invoked using admin auth (either directly or while
   * acting as an end user, e.g. from the dashboard), resolved to information
   * about the admin. Otherwise, resolved to `null`.
   */
  convexActor: CONVEX_ACTOR,
} as const;
