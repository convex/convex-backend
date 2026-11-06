import { describe, expect, test, vi } from "vitest";
import { OneoffCtx } from "../../bundler/context.js";
import { PushOptions } from "./components.js";
import { watchAndPush } from "./dev.js";

vi.mock("./components.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./components.js")>()),
  runPush: vi.fn(async () => {}),
}));
vi.mock("./config.js", async (importOriginal) => ({
  ...(await importOriginal<typeof import("./config.js")>()),
  readProjectConfig: vi.fn(async () => ({ projectConfig: {} })),
  getAuthKitConfig: vi.fn(async () => null),
}));

class ProcessExit extends Error {
  constructor(public exitCode: number) {
    super(`process.exit(${exitCode})`);
  }
}

describe("watchAndPush", () => {
  test("--once exits with code 1 when the --start command fails", async () => {
    const flushAndExit = vi.fn(async (exitCode: number): Promise<never> => {
      throw new ProcessExit(exitCode);
    });
    const ctx = {
      flushAndExit,
      registerCleanup: () => "handle",
      removeCleanup: () => null,
      bigBrainAuth: () => null,
    } as unknown as OneoffCtx;

    await expect(
      watchAndPush(ctx, {} as PushOptions, {
        run: {
          kind: "shell",
          command: `"${process.execPath}" -e "process.exit(7)"`,
        },
        once: true,
        untilSuccess: false,
        traceEvents: false,
      }),
    ).rejects.toThrow(new ProcessExit(1));
    expect(flushAndExit.mock.calls).toEqual([[1]]);
  });
});
