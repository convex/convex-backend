import { log } from "convex/server";
import { v } from "convex/values";
import { mutation } from "./_generated/server";
import { internal } from "./_generated/api";

export const updateDocument = mutation({
  args: {
    userId: v.string(),
    orgId: v.string(),
    documentId: v.id("documents"),
  },
  handler: async (ctx, { userId, orgId, documentId }) => {
    const document = await ctx.db.get("documents", documentId);

    const identity = await ctx.auth.getUserIdentity();
    const { name: deploymentName } = await ctx.meta.getDeploymentMetadata();
    const { name: functionName } = await ctx.meta.getFunctionMetadata();

    await log.audit({
      action: "document.viewed",
      actor: { userId, authUserId: identity?.sub },
      source: {
        ip: log.vars.ip,
        userAgent: log.vars.userAgent,
        deploymentName,
        functionName,
      },
      fields: {
        documentId,
        orgId,
        deploymentName,
        functionName,
      },
    });

    // These are available in code in mutations & actions,
    // so you can pass them along across scheduler boundaries, etc.
    // For queries, they will be available in logs but not in code.
    const { ip, userAgent, requestId } = await ctx.meta.getRequestMetadata();
    await ctx.scheduler.runAfter(0, internal.foo.bar, {
      requestMetadata: {
        ip,
        userAgent,
        requestId,
      },
      documentId,
    });

    return document;
  },
});
