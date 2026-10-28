import {
  internalQuery,
  internalAction,
  internalMutation,
} from "./_generated/server";
import { v } from "convex/values";
import { internal } from "./_generated/api";
import { hashSha256 } from "./util/hash";
import { getLatestCursorRules } from "./util/cursorRules";
import { Doc } from "./_generated/dataModel";

export const refresh = internalAction({
  args: {},
  handler: async (ctx): Promise<Doc<"cursorRules"> | null> => {
    try {
      const rules = await getLatestCursorRules();

      return await ctx.runMutation(internal.cursorRules.save, rules);
    } catch (error) {
      console.error("Failed to refresh Cursor rules:", error);
      return null;
    }
  },
});

export const getCached = internalQuery({
  args: {},
  handler: async (ctx) => {
    return await ctx.db.query("cursorRules").first();
  },
});

export const save = internalMutation({
  args: { content: v.string(), version: v.string() },
  handler: async (ctx, { content, version }) => {
    // Delete existing entries
    const existing = await ctx.db.query("cursorRules").collect();
    for (const entry of existing) {
      await ctx.db.delete("cursorRules", entry._id);
    }

    // Insert new entry
    const doc = await ctx.db.insert("cursorRules", {
      content,
      version,
      hash: await hashSha256(content),
    });
    return (await ctx.db.get("cursorRules", doc))!;
  },
});
