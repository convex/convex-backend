import { convexTest } from "convex-test";
import { expect, test } from "vitest";
import schema from "./schema";

test("functions", async () => {
  const t = convexTest(schema, modules);
  const firstTask = await t.run(async (ctx) => {
    await ctx.db.insert("tasks", { text: "Eat breakfast" });
    return await ctx.db.query("tasks").first();
  });
  expect(firstTask).toMatchObject({ text: "Eat breakfast" });
});

const modules = import.meta.glob("./**/*.ts");
