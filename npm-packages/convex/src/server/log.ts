import { audit } from "./audit_logging.js";
import { vars } from "./logVars.js";

// Type annotations are needed for the `unique symbol` types in `vars` to typecheck correctly
interface Log {
  /**
   * Emit a durable audit log. This functionality is only available for Convex
   * Enterprise (see https://www.convex.dev/enterprise/pricing).
   *
   * Use dynamic variables from `log.vars` to include deferred values that will
   * be resolved when emitting the log. Cached query hits will replay audit logs
   * with updated values.
   * ```ts
   * await log.audit({
   *   action: "document.viewed",
   *   actor: { userId },
   *   source: {
   *     ip: log.vars.ip,
   *     userAgent: log.vars.userAgent,
   *   },
   *   timestamp: log.vars.now,
   * });
   * ```
   *
   * The log body must be JSON-serializable.
   */
  audit: typeof audit;
  vars: typeof vars;
}

export const log: Log = {
  audit,
  vars,
};
