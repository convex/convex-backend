import { describe, expect, it } from "vitest";
import {
  nodeExecutorProcessTitle,
  setNodeExecutorProcessTitle,
} from "./processTitle";

describe("nodeExecutorProcessTitle", () => {
  it("uses the default title without an override", () => {
    expect(nodeExecutorProcessTitle()).toBe("convex-node-executor");
  });

  it("normalizes an inherited override", () => {
    expect(nodeExecutorProcessTitle("  local\tworker\n1  ")).toBe(
      "local worker 1",
    );
  });

  it("falls back to the default for an empty normalized override", () => {
    expect(nodeExecutorProcessTitle("\u0000 \n\t")).toBe(
      "convex-node-executor",
    );
  });

  it("caps overrides without splitting UTF-8 characters", () => {
    const title = nodeExecutorProcessTitle("\u{1F642}".repeat(25));
    expect(Buffer.byteLength(title)).toBe(96);
    expect(title).toBe("\u{1F642}".repeat(24));
  });

  it("sets the title on the provided process target", () => {
    const target = { title: "" };
    setNodeExecutorProcessTitle(target, "local executor");
    expect(target.title).toBe("local executor");
  });
});
