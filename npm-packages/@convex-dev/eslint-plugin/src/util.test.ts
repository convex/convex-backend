import path from "path";
import { describe, expect, test } from "vitest";
import { isEntryPoint } from "./util.js";

const file = (...segments: string[]) => path.join("/tmp", ...segments);

describe("isEntryPoint", () => {
  test("accepts a function file", () => {
    expect(isEntryPoint(file("app", "convex", "probe.ts"))).toBe(true);
  });

  test("ignores spaces in the project path", () => {
    expect(isEntryPoint(file("space dir", "app", "convex", "probe.ts"))).toBe(
      true,
    );
  });

  test("rejects spaces below the convex directory", () => {
    expect(isEntryPoint(file("app", "convex", "my dir", "probe.ts"))).toBe(
      false,
    );
    expect(isEntryPoint(file("app", "convex", "my probe.ts"))).toBe(false);
  });
});
