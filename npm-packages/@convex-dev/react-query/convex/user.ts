import { getAuthUserId } from "@convex-dev/auth/server";
import { Doc } from "./_generated/dataModel.js";
import { query, type QueryCtx } from "./_generated/server.js";

export const getCurrent = query({
  args: {},
  handler: async (ctx: QueryCtx): Promise<Doc<"users"> | null> => {
    const userId = await getAuthUserId(ctx);
    if (!userId) {
      throw new Error("Unauthorized");
    }
    return await ctx.db.get("users", userId);
  },
});
