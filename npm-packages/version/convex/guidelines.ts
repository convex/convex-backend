import {
  internalQuery,
  internalAction,
  internalMutation,
} from "./_generated/server";
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { hashSha256 } from "./util/hash";
import { getLatestGuidelines } from "./util/guidelines";
import { Doc } from "./_generated/dataModel";

export const refresh = internalAction({
  args: {},
  handler: async (ctx): Promise<Doc<"guidelines"> | null> => {
    try {
      const rules = await getLatestGuidelines();

      return await ctx.runMutation(internal.guidelines.save, rules);
    } catch (error) {
      console.error("Failed to refresh Guidelines:", error);
      return null;
    }
  },
});

export const getCached = internalQuery({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("guidelines").first();
  },
});

export const save = internalMutation({
  args: { content: v.string(), version: v.string() },
  handler: async (ctx, { content, version }) => {
    // Delete existing entries
    const existing = await ctx.db.query("guidelines").collect();
    for (const entry of existing) {
      await ctx.db.delete("guidelines", entry._id);
    }

    // Insert new entry
    const doc = await ctx.db.insert("guidelines", {
      content,
      version,
      hash: await hashSha256(content),
    });
    return (await ctx.db.get("guidelines", doc))!;
  },
});
