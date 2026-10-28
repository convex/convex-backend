import { v } from "convex/values";
import {
  DatabaseReader,
  internalMutation,
  mutation,
} from "./_generated/server";
import { internal } from "./_generated/api";

export const importDocuments = mutation({
  args: {
    docs: v.array(
      v.object({
        text: v.string(),
      }),
    ),
  },
  handler: async (ctx, args) => {
    const promises = [];
    for (const doc of args.docs) {
      promises.push(ctx.db.insert("documents", doc));
    }
    await Promise.all(promises);
  },
});

export const searchRandom = internalMutation({
  args: {},
  handler: async (ctx) => {
    const random = await _queryRandom(ctx.db);
    if (random !== null) {
      const results = await ctx.db
        .query("documents2")
        .withSearchIndex("by_text", (q) => q.search("text", random!.text))
        .take(10);
      console.debug("results", results);
    } else {
      console.debug("no results");
    }
    await ctx.scheduler.runAfter(
      350,
      internal.importDocuments.searchRandom,
      {},
    );
  },
});
async function _queryRandom(db: DatabaseReader) {
  const firstTs = (await db
    .query("documents")
    .withIndex("by_creation_time")
    .order("asc")
    .first())!._creationTime;
  const lastTs = (await db
    .query("documents")
    .withIndex("by_creation_time")
    .order("desc")
    .first())!._creationTime;
  const random = Math.random() * (lastTs - firstTs) + firstTs;
  console.log("Got random", random);

  return await db
    .query("documents")
    .withIndex("by_creation_time", (q) => q.gte("_creationTime", random))
    .first();
}

export const populateTableInternal = internalMutation({
  args: {
    startTime: v.float64(),
    endTime: v.float64(),
    insertedSoFar: v.int64(),
  },
  handler: async (ctx, args) => {
    const random = Math.random() * (args.endTime - args.startTime);

    const toInsert = await ctx.db
      .query("documents")
      .withIndex("by_creation_time", (q) => q.gte("_creationTime", random))
      .take(45);

    for (const doc of toInsert) {
      await ctx.db.insert("documents2", {
        text: doc.text,
      });
    }
    let updatedInserts = args.insertedSoFar + BigInt(toInsert.length);
    if (updatedInserts > 500) {
      const currentDbSize = await ctx.db.query("size").unique();
      const newSize =
        (currentDbSize?.size ?? BigInt(0)) + BigInt(updatedInserts);
      console.debug("Updating size to", newSize);
      if (currentDbSize) {
        await ctx.db.replace("size", currentDbSize._id, { size: newSize });
      } else {
        await ctx.db.insert("size", { size: newSize });
      }
      if (newSize > 3_000_000) {
        console.warn(`Reached ${newSize}, stopping`);
        return;
      }
      updatedInserts = BigInt(0);
    }
    console.log("Scheduling");
    await ctx.scheduler.runAfter(
      0,
      internal.importDocuments.populateTableInternal,
      {
        startTime: args.startTime,
        endTime: args.endTime,
        insertedSoFar: updatedInserts,
      },
    );
  },
});

export const populateTable = internalMutation({
  args: {},
  handler: async (ctx) => {
    const startTime = (await ctx.db
      .query("documents")
      .withIndex("by_creation_time")
      .order("asc")
      .first())!._creationTime;
    const endTime = (await ctx.db
      .query("documents")
      .withIndex("by_creation_time")
      .order("desc")
      .first())!._creationTime;
    await ctx.scheduler.runAfter(
      0,
      internal.importDocuments.populateTableInternal,
      {
        startTime,
        endTime,
        insertedSoFar: BigInt(0),
      },
    );
  },
});
