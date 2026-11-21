import { afterAll, beforeEach, describe, expect, test, vi } from "vitest";
import type { Context } from "../../../bundler/context.js";
import { ThrowingFetchError } from "../utils/utils.js";
import { bigBrainRecordActivity, bigBrainStart } from "./bigBrain.js";

// The CLI's retrying fetch captures the global `fetch` when utils.ts is
// imported, so the stub has to be installed before the imports above run.
const fetchMock = vi.hoisted(() => {
  const mock = vi.fn<typeof fetch>();
  vi.stubGlobal("fetch", mock);
  return mock;
});

afterAll(() => {
  vi.unstubAllGlobals();
});

function makeContext() {
  const crash = vi.fn<Context["crash"]>(async (args) => {
    return Promise.reject(new Error(`ctx.crash: ${args.printedMessage}`));
  });
  const ctx: Context = {
    fs: {} as Context["fs"],
    deprecationMessagePrinted: false,
    crash,
    registerCleanup: () => "handle",
    removeCleanup: () => async () => {},
    bigBrainAuth: () => null,
    _updateBigBrainAuth: () => {},
  };
  return { ctx, crash };
}

describe("bigBrainRecordActivity", () => {
  beforeEach(() => {
    fetchMock.mockReset();
  });

  test("throws on an HTTP error instead of crashing the CLI", async () => {
    fetchMock.mockResolvedValue(
      new Response("error code: 522", { status: 522 }),
    );
    const { ctx, crash } = makeContext();

    await expect(
      bigBrainRecordActivity(ctx, {
        instanceName: "local-test",
        adminKey: "test-admin-key",
      }),
    ).rejects.toBeInstanceOf(ThrowingFetchError);
    expect(crash).not.toHaveBeenCalled();
  });

  test("other local deployment calls still crash on HTTP errors", async () => {
    fetchMock.mockResolvedValue(
      new Response("error code: 522", { status: 522 }),
    );
    const { ctx, crash } = makeContext();

    await expect(
      bigBrainStart(ctx, {
        port: 3210,
        projectSlug: "project",
        teamSlug: "team",
        instanceName: null,
      }),
    ).rejects.toThrow("ctx.crash");
    expect(crash).toHaveBeenCalledOnce();
  });
});
