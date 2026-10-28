import { expect, test } from "vitest";
import schema from "./schema";
import { convexTest } from "convex-test";

// @snippet start overrideMath
test("overriding Math within a handler", async () => {
  const t = convexTest(schema, modules);
  const originalMath = globalThis.Math;

  const result = await t.run(async () => {
    const replacement: Math = Object.create(globalThis.Math);
    replacement.random = () => 0.5;
    globalThis.Math = replacement;

    return Math.random();
  });

  expect(result).toBe(0.5);
  expect(globalThis.Math).toBe(originalMath);
});
// @snippet end overrideMath

// @snippet start unsetCrypto
test("making crypto unavailable within a handler", async () => {
  const t = convexTest(schema, modules);

  const result = await t.run(async () => {
    (globalThis as Record<string, unknown>).crypto = undefined;

    return typeof crypto;
  });

  expect(result).toBe("undefined");
  expect(globalThis.crypto).toBeDefined();
});
// @snippet end unsetCrypto

const modules = import.meta.glob("./**/*.ts");
