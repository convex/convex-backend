import { log } from "convex/server";
import { api, components } from "./_generated/api";
import { mutation } from "./_generated/server";

export const loggedMutation = mutation({
  args: {},
  handler: async (_ctx) => {
    await log.audit({
      source: "loggedMutation",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
  },
});

export const loggedMutationInComponent = mutation({
  args: {},
  handler: async (ctx) => {
    await log.audit({
      source: "parentMutation",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
    await ctx.runMutation(components.myComponent.lib.loggedMutation, {});
  },
});

export const scheduleLoggedMutation = mutation({
  args: {},
  handler: async (ctx) => {
    await log.audit({
      source: "scheduleLoggedMutation",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
    await ctx.scheduler.runAfter(0, api.mutations.loggedMutation, {});
  },
});
