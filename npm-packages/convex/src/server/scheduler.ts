import {
  FunctionReference,
  FunctionReference_future,
  OptionalRestArgs,
} from "../server/api.js";
import { Id } from "../values/value.js";

/**
 * A {@link FunctionReference} that can be scheduled to run in the future.
 *
 * Schedulable functions are mutations and actions that are public or internal.
 *
 * @public
 */
export type SchedulableFunctionReference = FunctionReference<
  "mutation" | "action",
  "public" | "internal"
>;

// What the scheduling methods accept. This is kept separate from the exported
// `SchedulableFunctionReference` so that alias keeps describing a plain
// reference. Existing code depending on it can still forward it to anything
// typed with `FunctionReference` or read its `_args`. This alias is
// module-local so that a consumer's declaration file inlines it instead of
// importing it from a path that is not part of the package's public entry
// points.
type SchedulableFunctionReferenceCompat =
  | SchedulableFunctionReference
  | FunctionReference_future<"mutation" | "action", "public" | "internal">;

/**
 * An interface to schedule Convex functions to run in the future.
 *
 * Available as `ctx.scheduler` in mutations and actions.
 *
 * **Execution guarantees:**
 * - **Scheduled mutations** are guaranteed to execute **exactly once**. They
 *   are automatically retried on transient errors.
 * - **Scheduled actions** execute **at most once**. They are not retried and
 *   may fail due to transient errors.
 *
 * Consider using an `internalMutation` or `internalAction` to ensure that
 * scheduled functions cannot be called directly from a client.
 *
 * @example
 * ```typescript
 * import { mutation } from "./_generated/server";
 * import { internal } from "./_generated/api";
 * import { v } from "convex/values";
 *
 * export const createOrder = mutation({
 *   args: { items: v.array(v.string()) },
 *   returns: v.null(),
 *   handler: async (ctx, args) => {
 *     const orderId = await ctx.db.insert("orders", { items: args.items });
 *
 *     // Run immediately after this mutation commits:
 *     await ctx.scheduler.runAfter(0, internal.emails.sendConfirmation, {
 *       orderId,
 *     });
 *
 *     // Run cleanup in 7 days:
 *     await ctx.scheduler.runAfter(
 *       7 * 24 * 60 * 60 * 1000,
 *       internal.orders.archiveOrder,
 *       { orderId },
 *     );
 *
 *     return null;
 *   },
 * });
 * ```
 *
 * @see https://docs.convex.dev/scheduling/scheduled-functions
 * @public
 */
export interface Scheduler {
  /**
   * Schedule a function to execute after a delay.
   *
   * @example
   * ```typescript
   * // Schedule to run as soon as possible (if this is a mutation it would be after this mutation commits):
   * await ctx.scheduler.runAfter(0, internal.tasks.process, { taskId });
   *
   * // Run after 5 seconds:
   * await ctx.scheduler.runAfter(5000, internal.tasks.process, { taskId });
   *
   * // Run after 1 hour:
   * await ctx.scheduler.runAfter(60 * 60 * 1000, internal.cleanup.run, {});
   * ```
   *
   * @param delayMs - Delay in milliseconds. Must be non-negative. If the delay
   * is zero, the scheduled function will be due to execute immediately after the
   * scheduling one completes.
   * @param functionReference - A {@link FunctionReference} or
   * {@link FunctionReference_future} for the function to schedule.
   * @param args - Arguments to call the scheduled functions with.
   * @returns The ID of the scheduled function in the `_scheduled_functions`
   * system table. Use this to cancel it later if needed.
   **/
  runAfter<FuncRef extends SchedulableFunctionReferenceCompat>(
    delayMs: number,
    functionReference: FuncRef,
    ...args: OptionalRestArgs<FuncRef>
  ): Promise<Id<"_scheduled_functions">>;

  /**
   * Schedule a function to execute at a specific time.
   *
   * @example
   * ```typescript
   * // Run at a specific Date:
   * await ctx.scheduler.runAt(
   *   new Date("2030-01-01T00:00:00Z"),
   *   internal.events.triggerNewYear,
   *   {},
   * );
   *
   * // Run at a timestamp (milliseconds since epoch):
   * await ctx.scheduler.runAt(Date.now() + 60000, internal.tasks.process, { taskId });
   * ```
   *
   * @param timestamp - A Date or a timestamp (milliseconds since the epoch).
   * If the timestamp is in the past, the scheduled function will be due to
   * execute immediately after the scheduling one completes. The timestamp can't
   * be more than five years in the past or more than five years in the future.
   * @param functionReference - A {@link FunctionReference} or
   * {@link FunctionReference_future} for the function to schedule.
   * @param args - Arguments to call the scheduled functions with.
   * @returns The ID of the scheduled function in the `_scheduled_functions`
   * system table.
   **/
  runAt<FuncRef extends SchedulableFunctionReferenceCompat>(
    timestamp: number | Date,
    functionReference: FuncRef,
    ...args: OptionalRestArgs<FuncRef>
  ): Promise<Id<"_scheduled_functions">>;

  /**
   * Cancel a previously scheduled function.
   *
   * For scheduled **actions**: if the action has not started, it will
   * not run. If it is already in progress, it will continue running but any
   * new functions it tries to schedule will be canceled.
   * If it had already completed, canceling will throw an error.
   * For scheduled **mutations**: the mutation will either show up as
   * "pending", "completed", or "failed", but never "inProgress".
   * Canceling a mutation will atomically cancel it entirely or fail to cancel
   * if it has committed. It is a transaction that will either run to
   * completion and commit or fully roll back.
   *
   * @example
   * ```typescript
   * // Cancel a scheduled function:
   * await ctx.scheduler.cancel(scheduledFunctionId);
   * ```
   *
   * @param id - The ID of the scheduled function to cancel (returned by
   * `runAfter` or `runAt`).
   */
  cancel(id: Id<"_scheduled_functions">): Promise<void>;
}
