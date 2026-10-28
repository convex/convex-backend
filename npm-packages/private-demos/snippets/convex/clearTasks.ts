import { internalMutation } from "./_generated/server";
import { handleTaskDeletion } from "./lib/tasks";
import { internal } from "./_generated/api";

const MiB = 1 << 20;

export const clearTasks = internalMutation({
  args: {},
  handler: async (ctx, args) => {
    const tasks = ctx.db
      .query("tasks")
      .withIndex("by_status", (q) => q.eq("status", { archived: true }));

    for await (const task of tasks) {
      await handleTaskDeletion(ctx, task);
      await ctx.db.delete("tasks", task._id);
      const metrics = await ctx.meta.getTransactionMetrics();
      if (
        metrics.bytesRead.used > 4 * MiB ||
        metrics.bytesWritten.used > 2 * MiB ||
        metrics.databaseQueries.remaining < 500
      ) {
        // Run this mutation again and continue clearing tasks.
        await ctx.scheduler.runAfter(0, internal.clearTasks.clearTasks);
        break;
      }
    }
  },
});
