"use node";

import { RequestMetadata } from "convex/server";
import { api } from "./_generated/api";
import { action } from "./_generated/server";

// Direct request metadata access from node action
export const fromNodeAction = action({
  args: {},
  handler: async (ctx): Promise<RequestMetadata> => {
    return await ctx.meta.getRequestMetadata();
  },
});

// Node action calling nested mutation
export const fromNodeActionCallingMutation = action({
  args: {},
  handler: async (ctx): Promise<RequestMetadata> => {
    return await ctx.runMutation(api.requestMetadata.fromMutation, {});
  },
});

// Node action calling nested V8 action
export const fromNodeActionCallingV8Action = action({
  args: {},
  handler: async (ctx): Promise<RequestMetadata> => {
    return await ctx.runAction(api.requestMetadata.fromAction, {});
  },
});

// Node action calling nested node action
export const fromNodeActionCallingNodeAction = action({
  args: {},
  handler: async (ctx): Promise<RequestMetadata> => {
    return await ctx.runAction(api.nodeActions.fromNodeAction, {});
  },
});
