import { v } from "convex/values";
import { mutationWithSession, queryWithSession } from "./lib/sessions";

/**
 * Gets the name from the current session.
 */
export const get = queryWithSession({
  args: {},
  handler: async (ctx) => {
    // ctx.user is set in the queryWithSession wrapper.
    return ctx.user?.name ?? null;
  },
});

/**
 * Updates the name in the current session.
 */
export const set = mutationWithSession({
  args: {
    name: v.string(),
  },
  handler: async (ctx, { name }) => {
    if (ctx.user) {
      await ctx.db.patch("users", ctx.user._id, { name });
    } else {
      await ctx.db.insert("users", { name, sessionId: ctx.sessionId });
    }
  },
});
