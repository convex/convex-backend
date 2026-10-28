import { convexTest } from "convex-test";
import { expect, test } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";

test("authenticated functions", async () => {
  const t = convexTest(schema, modules);

  const asSarah = t.withIdentity({ name: "Sarah" });
  await asSarah.mutation(api.tasks.create, { text: "Add tests" });

  const sarahsTasks = await asSarah.query(api.tasks.list);
  expect(sarahsTasks).toMatchObject([{ text: "Add tests" }]);

  const asLee = t.withIdentity({ name: "Lee" });
  const leesTasks = await asLee.query(api.tasks.list);
  expect(leesTasks).toEqual([]);
});

const modules = import.meta.glob("./**/*.ts");
