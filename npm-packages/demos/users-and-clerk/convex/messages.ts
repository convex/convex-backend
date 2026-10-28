import { query } from "./_generated/server";
// @snippet start load-user
import { v } from "convex/values";
import { mutation } from "./_generated/server";

export const send = mutation({
  args: { body: v.string() },
  handler: async (ctx, args) => {
    const identity = await ctx.auth.getUserIdentity();
    if (!identity) {
      throw new Error("Unauthenticated call to mutation");
    }
    const user = await ctx.db
      .query("users")
      .withIndex("by_token", (q) =>
        q.eq("tokenIdentifier", identity.tokenIdentifier),
      )
      .unique();
    if (!user) {
      throw new Error("Unauthenticated call to mutation");
    }
    await ctx.db.insert("messages", { body: args.body, user: user._id });
  },
});
// @snippet end load-user

// @snippet start use-users
export const list = query({
  args: {},
  handler: async (ctx) => {
    const messages = await ctx.db.query("messages").collect();
    return Promise.all(
      messages.map(async (message) => {
        // For each message in this channel, fetch the `User` who wrote it and
        // insert their name into the `author` field.
        const user = await ctx.db.get("users", message.user);
        return {
          author: user?.name ?? "Anonymous",
          ...message,
        };
      }),
    );
  },
});
// @snippet end use-users
