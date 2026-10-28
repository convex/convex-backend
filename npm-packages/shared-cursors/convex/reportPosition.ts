import { Id } from "./_generated/dataModel";
import { mutation } from "./_generated/server";
import { v } from "convex/values";

export const reportContentiously = mutation({
  args: {
    x: v.number(),
    y: v.number(),
    ts: v.number(),
    session: v.string(),
    id: v.optional(v.any()),
  },
  handler: async ({ db }, { x, y, ts, session }): Promise<Id<"positions">> => {
    let pos = await db
      .query("positions")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- intentionally writing a query that causes contention
      .filter((q) => q.eq(q.field("session"), session))
      .first();
    if (pos === null) {
      pos = { session, x, y, clientSentTs: ts, serverSentTs: Date.now() };
      return db.insert("positions", pos);
    } else {
      await db.patch(pos._id, { x, y, ts, serverSentTs: Date.now() });
      return pos._id;
    }
  },
});

export const report = mutation({
  args: {
    x: v.number(),
    y: v.number(),
    ts: v.number(),
    session: v.string(),
    id: v.union(v.id("positions"), v.null()),
  },
  handler: async (
    { db },
    { x, y, ts, session, id },
  ): Promise<Id<"positions">> => {
    let pos = null;
    if (id !== null) {
      pos = await db.get(id);
    }
    if (pos === null) {
      pos = { session, x, y, clientSentTs: ts, serverSentTs: Date.now() };
      return await db.insert("positions", pos);
    } else {
      await db.patch(pos._id, { x, y, ts, serverSentTs: Date.now() });
      return pos._id;
    }
  },
});
