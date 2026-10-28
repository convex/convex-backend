import { v } from "convex/values";
import { GenericId } from "convex/values";
import { internalMutation, internalQuery } from "./_generated/server";

export const send = internalMutation({
  args: { author: v.id("users"), body: v.string() },
  handler: async (ctx, { author, body }) => {
    await ctx.db.insert("messages", { author, body });
  },
});

export const listByAuthor = internalQuery({
  args: { author: v.string() },
  handler: async (ctx, { author }) => {
    return await ctx.db
      .query("messages")
      .withIndex("by_author", (q) =>
        q.eq("author", author as GenericId<"users">),
      )
      .collect();
  },
});

export const clearAll = internalMutation({
  args: {},
  handler: async (ctx) => {
    for await (const message of ctx.db.query("messages")) {
      await ctx.db.delete("messages", message._id);
    }
  },
});
