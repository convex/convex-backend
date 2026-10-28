import { api } from "./_generated/api";
import { mutation } from "./_generated/server";
import { CommitTsPlaceholder, v } from "convex/values";

// @snippet start insert
export const enqueue = mutation({
  args: { payload: v.string() },
  handler: async (ctx, { payload }) => {
    await ctx.db.insert("work", {
      payload,
      updatedAt: ctx.db.vars.commitTs,
    });
  },
});
// @snippet end insert

// @snippet start queryPending
export const enqueueAndListPending = mutation({
  args: { payload: v.string() },
  handler: async (ctx, { payload }) => {
    await ctx.db.insert("work", {
      payload,
      updatedAt: ctx.db.vars.commitTs,
    });
    return await ctx.db
      .query("work")
      .withIndex("by_updatedAt", (q) => q.eq("updatedAt", ctx.db.vars.commitTs))
      .collect();
  },
});
// @snippet end queryPending

// @snippet start subtransaction
export const enqueueWithTimestamp = mutation({
  args: { payload: v.string(), updatedAt: v.commitTs() },
  returns: v.commitTs(),
  handler: async (ctx, { payload, updatedAt }) => {
    await ctx.db.insert("work", { payload, updatedAt });
    return updatedAt;
  },
});

export const enqueueViaSubtransaction = mutation({
  args: { payload: v.string() },
  returns: v.commitTs(),
  handler: async (ctx, { payload }): Promise<bigint | CommitTsPlaceholder> => {
    return await ctx.runMutation(api.commitTs.enqueueWithTimestamp, {
      payload,
      updatedAt: ctx.db.vars.commitTs,
    });
  },
});
// @snippet end subtransaction

export const checkCommitTs = mutation({
  args: { commitTs: v.commitTs() },
  handler: async (ctx, { commitTs }) => {
    // @snippet start placeholder
    if (commitTs instanceof CommitTsPlaceholder) {
      return "pending";
    }
    // commitTs is an Int64
    // @snippet end placeholder
    return commitTs;
  },
});
