import { convexTest } from "convex-test";
import { expect, test } from "vitest";
import schema from "./schema";

test("functions", async () => {
  const t = convexTest(schema, modules);
  const response = await t.fetch("/some/path", { method: "POST" });
  expect(response.status).toBe(200);
});

const modules = import.meta.glob("./**/*.ts");
