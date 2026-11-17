import { createFunctionHandle } from "convex/server";
import { v } from "convex/values";
import { components, internal } from "./_generated/api";
import { internalMutation, mutation } from "./_generated/server";

// @snippet start start
export const start = mutation({
  args: {},
  handler: async (ctx) => {
    await ctx.runMutation(components.jobs.lib.enqueue, {
      input: "hello",
      onComplete: await createFunctionHandle(internal.jobs.saveResult),
    });
    await ctx.runMutation(components.jobs.lib.enqueue, {
      input: "hello",
      // @ts-expect-error `saveCount` takes `count: number`, not `result: string`.
      onComplete: await createFunctionHandle(internal.jobs.saveCount),
    });
  },
});

export const saveResult = internalMutation({
  args: { jobId: v.string(), result: v.string() },
  handler: async () => {
    // ...
  },
});

export const saveCount = internalMutation({
  args: { jobId: v.string(), count: v.number() },
  handler: async () => {
    // ...
  },
});
// @snippet end start
