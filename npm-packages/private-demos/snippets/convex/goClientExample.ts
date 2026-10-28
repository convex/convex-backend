import { v } from "convex/values";
import { env, query } from "./_generated/server";
import { LinkTable } from "./schema";

export const loadOne = query({
  args: { normalizedId: v.string(), token: v.string() },
  returns: v.union(
    v.object({
      ...LinkTable.validator.fields,
      _creationTime: v.number(),
      _id: v.id("links"),
    }),
    v.null(),
  ),
  handler: async (ctx, { normalizedId, token }) => {
    if (token === "" || token !== env.CONVEX_AUTH_TOKEN) {
      throw new Error("Invalid authorization token");
    }
    return await ctx.db
      .query("links")
      .withIndex("by_normalizedId", (q) => q.eq("normalizedId", normalizedId))
      .first();
  },
});
