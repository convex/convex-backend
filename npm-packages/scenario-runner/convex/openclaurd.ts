import { v } from "convex/values";
import { DatabaseWriter, mutation, query } from "./_generated/server";
import randomWords from "random-words";
import { EMBEDDING_SIZE, OPENCLAURD_TABLE } from "../types";
import { CACHE_BREAKER_ARGS, RAND_MAX, rand } from "./common";
import { api } from "./_generated/api";

export const deleteOpenclaurd = mutation({
  args: {},
  handler: async (ctx) => {
    const toDelete = await ctx.runQuery(api.openclaurd.findRandomOpenclaurd, {
      cacheBreaker: Math.random(),
    });
    if (!toDelete) {
      return;
    }
    await ctx.db.delete("openclaurd", toDelete._id);
  },
});

export const insertOpenclaurdLowCardinality = mutation({
  args: {},
  handler: async ({ db }) => {
    await insertOpenclaurdHelper(db, 5);
  },
});

export const insertOpenclaurdHighCardinality = mutation({
  args: {},
  handler: async ({ db }) => {
    await insertOpenclaurdHelper(db, 100);
  },
});

async function insertOpenclaurdHelper(
  db: DatabaseWriter,
  userCardinality: number,
) {
  const text = randomWords({ exactly: 10 }).join(" ");

  const embedding = [];
  for (let j = 0; j < EMBEDDING_SIZE; j++) {
    embedding.push(Math.random());
  }

  await db.insert(OPENCLAURD_TABLE, {
    user: `user ${rand() % userCardinality}`,
    timestamp: Date.now(),
    text,
    rand: rand(),
    embedding,
  });
}

export const findRandomOpenclaurd = query({
  args: CACHE_BREAKER_ARGS,
  handler: async ({ db }) => {
    const lowestDoc = await db
      .query("openclaurd")
      .withIndex("by_rand")
      .order("asc")
      .first();
    if (!lowestDoc) {
      return null;
    }
    const highestDoc = await db
      .query("openclaurd")
      .withIndex("by_rand")
      .order("desc")
      .first();
    if (!highestDoc) {
      return null;
    }

    // eslint-disable-next-line @convex-dev/no-time-or-random-in-queries -- This benchmark deliberately selects a random result.
    const percentage = rand() / RAND_MAX;
    const range = highestDoc.rand - lowestDoc.rand;
    const targetItemRand = percentage * range + lowestDoc.rand;
    return await db
      .query("openclaurd")
      .withIndex("by_rand", (q) => q.gte("rand", targetItemRand))
      .first();
  },
});

export const queryOpenclaurdById = query({
  args: {
    id: v.id("openclaurd"),
  },
  handler: async (ctx, args) => {
    return await ctx.db.get("openclaurd", args.id);
  },
});
