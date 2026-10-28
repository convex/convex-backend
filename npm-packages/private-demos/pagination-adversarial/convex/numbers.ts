import { paginationOptsValidator } from "convex/server";
import { mutation, query } from "./_generated/server";
import { v } from "convex/values";

export const count = mutation({
  args: {},
  handler: async ({ db }) => {
    const bigNumber =
      (await db.query("numbers").withIndex("number").order("desc").first())
        ?.number ?? 0;
    for (let i = 1; i <= 10; i++) {
      await db.insert("numbers", { number: bigNumber + i, id: i.toString() });
    }
  },
});

export const insert = mutation({
  args: { number: v.number(), id: v.string() },
  handler: async (ctx, args) => {
    await ctx.db.insert("numbers", {
      number: args.number,
      id: args.id,
    });
  },
});

export const reset = mutation({
  args: {},
  handler: async (ctx) => {
    const allNumbers = await ctx.db.query("numbers").collect();
    for (const number of allNumbers) {
      await ctx.db.delete("numbers", number._id);
    }
    await ctx.db.insert("numbers", { number: 0, id: crypto.randomUUID() });
    await ctx.db.insert("numbers", { number: 75, id: crypto.randomUUID() });
  },
});

export const listFilteredNumbers = query({
  args: { paginationOpts: paginationOptsValidator },
  handler: async (ctx, args) => {
    const results = await ctx.db
      .query("numbers")
      .withIndex("number")
      .order("desc")
      .paginate(args.paginationOpts);
    const numbers = results.page.map((d) => d.number);
    const filtered = numbers.filter((n) => n % 100 === 0);
    return {
      ...results,
      page: filtered.map((n, i) => {
        if (i === 0) {
          return `${n} -- start page`;
        }
        return n;
      }),
    };
  },
});

export const listNumbers = query({
  args: {
    paginationOpts: paginationOptsValidator,
    sortOrder: v.union(v.literal("asc"), v.literal("desc")),
  },
  handler: async (ctx, args) => {
    const results = await ctx.db
      .query("numbers")
      .withIndex("number")
      .order(args.sortOrder)
      .paginate(args.paginationOpts);
    return {
      ...results,
      page: results.page.map((d, i) => ({
        ...d,
        isStart: i === 0,
        isEnd: i === results.page.length - 1,
      })),
    };
  },
});
