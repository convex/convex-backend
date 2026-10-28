import { query } from "./_generated/server";
// @snippet start current-user
import { v } from "convex/values";
import { mutation } from "./_generated/server";
import { getCurrentUserOrThrow } from "./users";

export const send = mutation({
  args: { body: v.string() },
  handler: async (ctx, args) => {
    const user = await getCurrentUserOrThrow(ctx);
    await ctx.db.insert("messages", { body: args.body, userId: user._id });
  },
});
// @snippet end current-user

export const list = query({
  args: {},
  handler: async (ctx) => {
    const messages = await ctx.db.query("messages").collect();
    return Promise.all(
      messages.map(async (message) => {
        // For each message in this channel, fetch the `User` who wrote it and
        // insert their name into the `author` field.
        const user = await ctx.db.get("users", message.userId);
        return {
          author: user?.name ?? "deleted user",
          ...message,
        };
      }),
    );
  },
});

export const sentCount = query({
  args: {},
  handler: async (ctx) => {
    const user = await getCurrentUserOrThrow(ctx);
    const sent = await ctx.db
      .query("messages")
      .withIndex("byUserId", (q) => q.eq("userId", user._id))
      .collect();
    return sent.length;
  },
});
