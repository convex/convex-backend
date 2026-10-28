import { api } from "./_generated/api";
import { action } from "./_generated/server";

export const actionCallingLoggedFns = action({
  args: {},
  handler: async (ctx) => {
    await ctx.runQuery(api.queries.loggedQuery, {});
    await ctx.runMutation(api.mutations.loggedMutation, {});
  },
});
