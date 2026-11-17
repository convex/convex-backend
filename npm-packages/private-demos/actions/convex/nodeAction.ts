"use node";
import { api } from "./_generated/api";
import { action } from "./_generated/server";

export const nodeAction = action({
  handler: async () => {
    console.log("running lil node action");
  },
});

export const actionCallAction = action({
  handler: async (ctx) => {
    console.log("running action first time");
    await ctx.runAction(api.nodeAction.nodeAction);
    console.log("running action second time");
    await ctx.runAction(api.nodeAction.nodeAction);
  },
});

export const nodeVersion = action({
  handler: async () => {
    return process.version;
  },
});
