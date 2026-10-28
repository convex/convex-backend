import { v } from "convex/values";
import { internalMutation } from "./_generated/server";
import { internal } from "./_generated/api";

const MiB = 1 << 20;

// Reserve 1 MiB of reads and writes plus 100 document reads/writes for the
// status update below, by capping the nested call at `remaining - reserve`.
// Subtracting from `remaining` adapts to whatever the parent has already
// used, so the parent is always guaranteed enough budget for the cleanup —
// even if processTask consumes its entire quota or throws.
export const runWithStatus = internalMutation({
  args: { id: v.id("tasks") },
  handler: async (ctx, { id }) => {
    const metrics = await ctx.meta.getTransactionMetrics();
    const RESERVE_BYTES = 1 * MiB;
    const RESERVE_DOCS = 100;
    try {
      const result = await ctx.runMutation(
        internal.transactionLimits.processTask,
        { id },
        {
          transactionLimits: {
            bytesRead: metrics.bytesRead.remaining - RESERVE_BYTES,
            bytesWritten: metrics.bytesWritten.remaining - RESERVE_BYTES,
            documentsRead: metrics.documentsRead.remaining - RESERVE_DOCS,
            documentsWritten: metrics.documentsWritten.remaining - RESERVE_DOCS,
          },
        },
      );
      await ctx.db.patch("tasks", id, {
        result: { kind: "success", result },
      });
    } catch (e: any) {
      // The nested mutation's writes rolled back since it threw an exception, but
      // this parent mutation can still commit the error to the database.
      await ctx.db.patch("tasks", id, {
        result: { kind: "error", error: e?.message ?? String(e) },
      });
    }
  },
});

export const processTask = internalMutation({
  args: { id: v.id("tasks") },
  handler: async (_ctx, _args) => {
    // ...
  },
});
