import { internalMutation, mutation, query } from "./_generated/server";
import { v } from "convex/values";

export const list = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("messages").collect();
  },
});

export const send = mutation({
  args: {
    body: v.string(),
    author: v.string(),
  },
  handler: async (ctx, { body, author }) => {
    const message = { body, author };
    await ctx.db.insert("messages", message);
  },
});

export const clearAll = internalMutation({
  args: {},
  handler: async (ctx) => {
    for (const message of await ctx.db.query("messages").collect()) {
      await ctx.db.delete("messages", message._id);
    }
  },
});
