import { v } from "convex/values";
import {
  internalAction,
  internalMutation,
  internalQuery,
} from "./_generated/server";
import { internal } from "./_generated/api";
import { Doc } from "./_generated/dataModel";
import { fetchLatestNpmVersion } from "./util/npm";

export const refresh = internalAction({
  args: {},
  handler: async (ctx): Promise<Doc<"npmVersion"> | null> => {
    try {
      const version = await fetchLatestNpmVersion();

      return await ctx.runMutation(internal.npm.save, {
        version,
      });
    } catch (error) {
      console.error("Failed to refresh NPM version:", error);
      return null;
    }
  },
});

export const getCached = internalQuery({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("npmVersion").first();
  },
});

export const save = internalMutation({
  args: { version: v.string() },
  handler: async (ctx, args) => {
    // Delete existing entries
    const existing = await ctx.db.query("npmVersion").collect();
    for (const entry of existing) {
      await ctx.db.delete("npmVersion", entry._id);
    }

    // Insert new entry
    const doc = await ctx.db.insert("npmVersion", { value: args.version });
    return (await ctx.db.get("npmVersion", doc))!;
  },
});
