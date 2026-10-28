import { mutation } from "./_generated/server";

export const clientInfo = mutation({
  args: {},
  handler: async (ctx) => {
    const { ip, userAgent } = await ctx.meta.getRequestMetadata();
    return { ip, userAgent };
  },
});
