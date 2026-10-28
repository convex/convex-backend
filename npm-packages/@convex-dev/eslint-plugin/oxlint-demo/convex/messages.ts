import { v } from "convex/values";
import { mutation, query } from "./_generated/server";

// oxlint-disable-next-line @convex-dev/no-old-registered-function-syntax, @convex-dev/require-args-validator
export const listAll = query(async (ctx) => {
  return await ctx.db.query("messages").collect();
});

// oxlint-disable-next-line @convex-dev/require-args-validator
export const listByAuthor = query({
  handler: async (ctx, { author }: { author: string }) => {
    return await ctx.db
      .query("messages")
      // oxlint-disable-next-line @convex-dev/no-filter-in-query
      .filter((q) => q.eq(q.field("author"), author))
      .collect();
  },
});

export const remove = mutation({
  args: { id: v.id("messages") },
  handler: async (ctx, { id }) => {
    // oxlint-disable-next-line @convex-dev/explicit-table-ids
    await ctx.db.delete(id);
  },
});

export const greeting = query({
  args: {},
  handler: async () => {
    // oxlint-disable-next-line @convex-dev/no-process-env
    return process.env.GREETING ?? "hello";
  },
});
