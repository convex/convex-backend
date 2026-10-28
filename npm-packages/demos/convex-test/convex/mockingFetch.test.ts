import { expect, test, vi } from "vitest";
import { api } from "./_generated/api";
import schema from "./schema";
import { convexTest } from "convex-test";

test("ai", async () => {
  const t = convexTest(schema, modules);

  vi.stubGlobal(
    "fetch",
    vi.fn(async () => ({ text: async () => "I am the overlord" }) as Response),
  );

  const reply = await t.action(api.messages.sendAIMessage, { prompt: "hello" });
  expect(reply).toEqual("I am the overlord");

  vi.unstubAllGlobals();
});

const modules = import.meta.glob("./**/*.ts");
