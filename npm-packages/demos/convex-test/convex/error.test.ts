import { convexTest } from "convex-test";
import { expect, test } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";

test("messages validation", async () => {
  const t = convexTest(schema, modules);
  await expect(async () => {
    await t.mutation(api.messages.send, { body: "", author: "James" });
  }).rejects.toThrowError("Empty message body is not allowed");
});

const modules = import.meta.glob("./**/*.ts");
