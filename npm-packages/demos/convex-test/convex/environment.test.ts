import { describe, expect, it } from "vitest";

describe("convex test environment", () => {
  it("does not expose a jsdom document global", () => {
    const globals = globalThis as typeof globalThis & { document?: unknown };
    expect(globals.document).toBeUndefined();
  });
});
