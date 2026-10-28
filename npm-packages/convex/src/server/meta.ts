import { FunctionType } from "./api.js";
import { FunctionVisibility } from "./registration.js";

/**
 * Used and remaining amounts for a single transaction limit.
 *
 * @public
 */
export type TransactionMetric = {
  used: number;
  remaining: number;
};

/**
 * The remaining headroom for a transaction before hitting limits.
 *
 * See https://docs.convex.dev/production/state/limits
 *
 * @public
 */
export type TransactionMetrics = {
  bytesRead: TransactionMetric;
  bytesWritten: TransactionMetric;
  databaseQueries: TransactionMetric;
  documentsRead: TransactionMetric;
  documentsWritten: TransactionMetric;
  functionsScheduled: TransactionMetric;
  scheduledFunctionArgsBytes: TransactionMetric;
  filesWritten: TransactionMetric;
  fileWriteBytes: TransactionMetric;
  filesRead: TransactionMetric;
  fileReadBytes: TransactionMetric;
};

/**
 * Custom limits for a nested transaction. Each field specifies the absolute
 * maximum allowed for the nested function call. Values are capped at the
 * global transaction limits, so they can only lower limits, never raise them.
 *
 * @public
 */
export interface TransactionLimits {
  bytesRead?: number;
  bytesWritten?: number;
  databaseQueries?: number;
  documentsRead?: number;
  documentsWritten?: number;
  functionsScheduled?: number;
  scheduledFunctionArgsBytes?: number;
  filesWritten?: number;
  fileWriteBytes?: number;
  filesRead?: number;
  fileReadBytes?: number;
}

/**
 * Metadata about the currently executing Convex function.
 *
 * @public
 */
export type FunctionMetadata = {
  /**
   * The name of the function, in the format `"path/to/module:functionName"`
   */
  name: string;
  /**
   * The path of the component this function belongs to.
   * This is an empty string `""` for the app.
   */
  componentPath: string;
  /** Whether it's a query, mutation, or action. */
  type: FunctionType;
  /** Whether the function is public or internal. */
  visibility: FunctionVisibility;
};

/**
 * Metadata about the deployment this function is running on.
 *
 * @public
 */
export type DeploymentMetadata = {
  /**
   * The deployment name, e.g. `"tall-tiger-123"` for cloud deployments,
   * `"local-my_team-my_project"` for local deployments, or
   * `"anonymous-*"` for anonymous deployments.
   */
  name: string;
  /**
   * The deployment region, e.g. `"aws-us-east-1"`.
   * `null` for local and self-hosted deployments.
   */
  region: string | null;
  /**
   * The deployment class, e.g. `"s16"`, `"s256"`, `"d1024"`, or `"d2048"`.
   */
  class: "s16" | "s256" | "d1024" | "d2048";
};

/**
 * Metadata about the HTTP request that triggered the current function execution.
 *
 * `ip` and `userAgent` are `null` when the function was not triggered by an
 * HTTP request (e.g. scheduled jobs or cron jobs).
 *
 * Functions called from within a function (i.e. using `runMutation` or
 * `runAction`) will have the same request metadata as the parent function.
 *
 * @public
 */
export type RequestMetadata = {
  ip: string | null;
  userAgent: string | null;
  requestId: string;
  /**
   * The ID of the scheduled function document (in `_scheduled_functions`) that
   * this execution belongs to, or `null` otherwise.
   *
   * This is set for the scheduled function itself and for any functions it
   * calls (e.g. a mutation invoked via `runMutation` by a scheduled action),
   * propagating the top-level scheduled function's ID down the call tree. It is
   * `null` when the function was not scheduled.
   */
  scheduledFunctionId: string | null;
  /**
   * The raw auth token (a JWT) the request was authenticated with, or `null`
   * when the request was unauthenticated or authenticated with an admin key.
   *
   * This is the same token that `ctx.auth.getUserIdentity()` derives its
   * attributes from.
   */
  authToken: string | null;
};

/**
 * Extra context available in Convex query functions.
 *
 * @public
 */
export interface QueryMeta {
  /**
   * Metadata about the currently executing Convex function.
   */
  getFunctionMetadata(): Promise<FunctionMetadata>;
  /**
   * The remaining headroom for a transaction before hitting limits.
   *
   * See https://docs.convex.dev/production/state/limits
   */
  getTransactionMetrics(): Promise<TransactionMetrics>;
  /**
   * Metadata about the deployment this function is running on.
   */
  getDeploymentMetadata(): Promise<DeploymentMetadata>;
  /**
   * Returns the timestamp of the database snapshot this transaction reads
   * from, in nanoseconds.
   *
   * All commits at or before this timestamp are observable within the
   * transaction, and no later commits are. The value is fixed for the
   * lifetime of the transaction and shared with all nested `runMutation` and
   * non-stale `runQuery` calls. If a nested query is called with
   * `useStaleSnapshot: true`, then the nested query may in a future backend
   * version choose an older snapshotTs.
   *
   * It is on the same clock as `db.vars.commitTs`: documents observable in
   * this transaction have `commitTs` values at or before this timestamp, and
   * no new documents will be written with a `commitTs` at or below this,
   * including from the current transaction. When reading documents with an
   * index in commitTs order, you can use this as an upper bound to prevent
   * conflicting with racing inserts.
   *
   * Since the timestamp differs on every execution, calling this in a query
   * limits caching of the query's result the same way `Date.now()` does.
   *
   * Note: this should not be compared to `_creationTime` or `Date.now()`, as
   * those are based on wall-clock time rather than the database clock, and
   * aren't guaranteed to follow commit order.
   *
   * Docs: https://docs.convex.dev/database/advanced/commit-timestamp
   */
  getSnapshotTs(): bigint;
}

/**
 * Extra context available in Convex mutation functions.
 *
 * @public
 */
export interface MutationMeta extends QueryMeta {
  /**
   * Metadata about the HTTP request that triggered the current function execution.
   *
   * `ip` and `userAgent` are `null` when the function was not triggered by an
   * HTTP request (e.g. scheduled jobs or cron jobs).
   *
   * Functions called from within a function (i.e. using `runMutation`) will have
   * the same request metadata as the parent function.
   */
  getRequestMetadata(): Promise<RequestMetadata>;
}

/**
 * Extra context available in Convex action functions.
 *
 * @public
 */
export interface ActionMeta {
  /**
   * Metadata about the currently executing Convex function.
   */
  getFunctionMetadata(): Promise<FunctionMetadata>;
  /**
   * Metadata about the deployment this function is running on.
   */
  getDeploymentMetadata(): Promise<DeploymentMetadata>;
  /**
   * Metadata about the HTTP request that triggered the current function execution.
   *
   * `ip` and `userAgent` are `null` when the function was not triggered by an
   * HTTP request (e.g. scheduled jobs or cron jobs).
   *
   * Functions called from within a function (i.e. using `runMutation` or
   * `runAction`) will have the same request metadata as the parent function.
   */
  getRequestMetadata(): Promise<RequestMetadata>;
}
