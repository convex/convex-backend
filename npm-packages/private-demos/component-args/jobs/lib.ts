import { v } from "convex/values";
import { mutation, query } from "./_generated/server";

// @snippet start enqueue
export const enqueue = mutation({
  args: {
    input: v.string(),
    onComplete: v.function({
      fnType: "mutation",
      args: { jobId: v.string(), result: v.string() },
      returns: v.null(),
    }),
  },
  handler: async () => {
    // ...
  },
});
// @snippet end enqueue

// @snippet start hookFunctions
export const addHook = mutation({
  args: { handle: v.function() },
  handler: async (ctx, { handle }) => {
    return await ctx.db.insert("hooks", { handle });
  },
});

export const listHooks = query({
  args: {},
  returns: v.array(v.function()),
  handler: async (ctx) => {
    const hooks = await ctx.db.query("hooks").collect();
    return hooks.map((hook) => hook.handle);
  },
});
// @snippet end hookFunctions

export const runHooks = mutation({
  args: { event: v.string() },
  handler: async (ctx, { event }) => {
    const hooks = await ctx.db.query("hooks").collect();
    for (const { handle } of hooks) {
      // @snippet start callHandle
      await ctx.runMutation(handle, { event });
      // or run it asynchronously via the scheduler:
      await ctx.scheduler.runAfter(0, handle, { event });
      // @snippet end callHandle
    }
  },
});
