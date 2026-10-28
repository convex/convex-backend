import { v } from "convex/values";
import {
  internalAction,
  internalMutation,
  internalQuery,
} from "./_generated/server";
import { internal } from "./_generated/api";
import { Doc } from "./_generated/dataModel";
import { getLatestLocalBackendVersion } from "./util/localBackend";

export const refresh = internalAction({
  args: {},
  handler: async (ctx): Promise<Doc<"localBackendVersion"> | null> => {
    try {
      const version = await getLatestLocalBackendVersion();

      return await ctx.runMutation(internal.localBackend.save, {
        version,
      });
    } catch (error) {
      console.error("Failed to refresh local backend version:", error);
      return null;
    }
  },
});

export const getCached = internalQuery({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("localBackendVersion").first();
  },
});

export const save = internalMutation({
  args: { version: v.string() },
  handler: async (ctx, args) => {
    // Delete existing entries
    const existing = await ctx.db.query("localBackendVersion").collect();
    for (const entry of existing) {
      await ctx.db.delete("localBackendVersion", entry._id);
    }

    // Insert new entry
    const doc = await ctx.db.insert("localBackendVersion", {
      version: args.version,
    });
    return (await ctx.db.get("localBackendVersion", doc))!;
  },
});
