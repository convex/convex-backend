import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";
import { defineTestApp } from "convex-test";
import { expect, test } from "vitest";
import { Workpool } from "@convex-dev/workpool";
import workpoolTest from "@convex-dev/workpool/test";

const app = defineTestApp({
  schema: defineSchema({
    events: defineTable({ key: v.string() }),
  }),
  components: {
    myWorkpool: workpoolTest,
  },
});

const { api, createTest } = app.defineModules({
  events: {
    add: app.mutation({
      args: { key: v.string() },
      returns: v.id("events"),
      handler: async (ctx, args) => await ctx.db.insert("events", args),
    }),
  },
});

test("adds an event", async () => {
  const t = createTest();
  const id = await t.mutation(api.events.add, { key: "example" });
  const event = await t.query(async (ctx) => await ctx.db.get("events", id));
  expect(event?.key).toBe("example");
});

export const testWorkpool = new Workpool(app.components.myWorkpool, {});
