import randomWords from "random-words";
import { Doc } from "./_generated/dataModel";
import { mutation, query } from "./_generated/server";
import { CACHE_BREAKER_ARGS, rand, RAND_MAX } from "./common";
import { api } from "./_generated/api";

export const deleteMessageWithSearch = mutation({
  args: {},
  handler: async (ctx) => {
    const toDelete = await ctx.runQuery(api.search.findRandomSearchDocument, {
      cacheBreaker: Math.random(),
    });
    if (!toDelete) {
      return;
    }
    await ctx.db.delete("messages_with_search", toDelete._id);
  },
});

export const replaceMessageWithSearch = mutation({
  args: {},
  handler: async (ctx) => {
    const toReplace = await ctx.runQuery(api.search.findRandomSearchDocument, {
      cacheBreaker: Math.random(),
    });
    if (!toReplace) {
      return;
    }
    await ctx.db.replace("messages_with_search", toReplace._id, {
      channel: "global",
      timestamp: toReplace.timestamp,
      rand: rand(),
      body: randomWords({ exactly: 1 }).join(" "),
      ballastArray: [],
    });
  },
});

export default query({
  handler: async (
    { db },
    {
      channel,
      body,
      limit,
    }: {
      channel: string;
      body: string;
      limit: number;
    },
  ): Promise<Doc<"messages_with_search">[]> => {
    return await db
      .query("messages_with_search")
      .withSearchIndex("search_body", (q) =>
        q.search("body", body).eq("channel", channel),
      )
      .take(limit);
  },
});

export const findRandomSearchDocument = query({
  args: CACHE_BREAKER_ARGS,
  handler: async ({ db }) => {
    const lowestDoc = await db
      .query("messages_with_search")
      .withIndex("by_rand")
      .order("asc")
      .first();
    if (!lowestDoc) {
      return null;
    }
    const highestDoc = await db
      .query("messages_with_search")
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
      .query("messages_with_search")
      .withIndex("by_rand", (q) => q.gte("rand", targetItemRand))
      .first();
  },
});
