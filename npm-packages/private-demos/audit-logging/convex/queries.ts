import { log } from "convex/server";
import { components } from "./_generated/api";
import { query } from "./_generated/server";

export const loggedQuery = query({
  args: {},
  handler: async (_ctx) => {
    await log.audit({
      source: "loggedQuery",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
  },
});

export const loggedQueryWithConvexActor = query({
  args: {},
  handler: async (_ctx) => {
    await log.audit({
      source: "loggedQueryWithConvexActor",
      convexActor: log.vars.convexActor,
    });
  },
});

export const loggedQueryInComponent = query({
  args: {},
  handler: async (ctx) => {
    await log.audit({
      source: "parentQuery",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
    await ctx.runQuery(components.myComponent.lib.loggedQuery, {});
  },
});
