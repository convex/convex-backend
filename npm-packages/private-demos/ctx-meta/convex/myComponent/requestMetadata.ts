import { RequestMetadata } from "convex/server";
import { mutation, action } from "./_generated/server";

export const fromMutation = mutation({
  args: {},
  handler: async (ctx): Promise<RequestMetadata> => {
    return await ctx.meta.getRequestMetadata();
  },
});

export const fromAction = action({
  args: {},
  handler: async (ctx): Promise<RequestMetadata> => {
    return await ctx.meta.getRequestMetadata();
  },
});
