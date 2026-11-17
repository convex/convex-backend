import { v } from "convex/values";
import { mutation } from "./_generated/server";

// @snippet start storeInMutation
export const exportTasks = mutation({
  args: {},
  handler: async (ctx) => {
    const tasks = await ctx.db.query("tasks").take(100);
    const json = JSON.stringify(tasks.map((task) => task.text ?? ""));

    // Store the file in Convex
    const storageId = await ctx.storage.store(
      new Blob([json], { type: "application/json" }),
    );

    // Write `storageId` to a document in the same transaction
    await ctx.db.insert("exports", { storageId });
  },
});
// @snippet end storeInMutation

// @snippet start getInMutation
export const importTasks = mutation({
  args: { storageId: v.id("_storage") },
  handler: async (ctx, args) => {
    const blob = await ctx.storage.get(args.storageId);
    if (blob === null) {
      throw new Error("File not found");
    }
    const texts: string[] = JSON.parse(await blob.text());
    for (const text of texts) {
      await ctx.db.insert("tasks", { text });
    }
  },
});
// @snippet end getInMutation
