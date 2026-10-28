import { log } from "convex/server";
import { mutation, query } from "./_generated/server";

export const loggedMutation = mutation({
  args: {},
  handler: async (_ctx) => {
    await log.audit({
      source: "loggedMutationInComponent",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
  },
});

export const loggedQuery = query({
  args: {},
  handler: async (_ctx) => {
    await log.audit({
      source: "loggedQueryInComponent",
      requestId: log.vars.requestId,
      ip: log.vars.ip,
      userAgent: log.vars.userAgent,
      now: log.vars.now,
    });
  },
});
