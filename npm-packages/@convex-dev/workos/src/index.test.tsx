import type { ReactElement } from "react";
import { LoginRequiredError } from "@workos-inc/authkit-react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ConvexProviderWithAuthKit } from "./index.js";

vi.mock("react", async (importOriginal) => {
  const react = await importOriginal<typeof import("react")>();
  return {
    ...react,
    useCallback: <T,>(callback: T) => callback,
    useMemo: <T,>(factory: () => T) => factory(),
  };
});

type GetAccessToken = (options?: {
  forceRefresh?: boolean;
}) => Promise<string | null>;

type Adapter = {
  fetchAccessToken(args: {
    forceRefreshToken: boolean;
  }): Promise<string | null>;
};

function makeFetchAccessToken(getAccessToken: GetAccessToken) {
  const element = ConvexProviderWithAuthKit({
    children: null,
    client: {
      setAuth: () => {},
      clearAuth: () => {},
    },
    useAuth: () => ({
      isLoading: false,
      user: {},
      getAccessToken,
    }),
  }) as ReactElement<{ useAuth: () => Adapter }>;

  return element.props.useAuth().fetchAccessToken;
}

describe("ConvexProviderWithAuthKit", () => {
  afterEach(() => vi.useRealTimers());
  it("returns null when AuthKit requires login", async () => {
    const getAccessToken = vi
      .fn<GetAccessToken>()
      .mockRejectedValue(new LoginRequiredError());
    const fetchAccessToken = makeFetchAccessToken(getAccessToken);

    await expect(
      fetchAccessToken({ forceRefreshToken: false }),
    ).resolves.toBeNull();
    expect(getAccessToken).toHaveBeenCalledTimes(1);
  });

  it.each([false, true])(
    "recovers a transient failure (forced: %s)",
    async (forceRefreshToken) => {
      vi.useFakeTimers();
      const networkError = new TypeError("Failed to fetch");
      const getAccessToken = vi
        .fn<GetAccessToken>()
        .mockRejectedValueOnce(networkError)
        .mockResolvedValue("recovered-token");
      const fetchAccessToken = makeFetchAccessToken(getAccessToken);

      const result = fetchAccessToken({ forceRefreshToken });
      await vi.advanceTimersByTimeAsync(250);
      await expect(result).resolves.toBe("recovered-token");
      expect(getAccessToken).toHaveBeenCalledTimes(2);
      for (const args of getAccessToken.mock.calls) {
        expect(args).toEqual(forceRefreshToken ? [{ forceRefresh: true }] : []);
      }
    },
  );

  it("settles persistent failures without rejecting or retrying forever", async () => {
    vi.useFakeTimers();
    const getAccessToken = vi
      .fn<GetAccessToken>()
      .mockRejectedValue(new TypeError("offline"));
    const result = makeFetchAccessToken(getAccessToken)({
      forceRefreshToken: true,
    });
    await vi.advanceTimersByTimeAsync(749);
    expect(getAccessToken).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(1);
    await expect(result).resolves.toBeNull();
    expect(getAccessToken).toHaveBeenCalledTimes(3);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("stops retrying if a later attempt requires login", async () => {
    vi.useFakeTimers();
    const getAccessToken = vi
      .fn<GetAccessToken>()
      .mockRejectedValueOnce(new TypeError("offline"))
      .mockRejectedValue(new LoginRequiredError());
    const result = makeFetchAccessToken(getAccessToken)({
      forceRefreshToken: true,
    });
    await vi.advanceTimersByTimeAsync(250);
    await expect(result).resolves.toBeNull();
    expect(getAccessToken).toHaveBeenCalledTimes(2);
    expect(vi.getTimerCount()).toBe(0);
  });

  it("forces AuthKit to refresh when Convex requests it", async () => {
    const getAccessToken = vi
      .fn<GetAccessToken>()
      .mockResolvedValue("fresh-token");
    const fetchAccessToken = makeFetchAccessToken(getAccessToken);

    await expect(fetchAccessToken({ forceRefreshToken: true })).resolves.toBe(
      "fresh-token",
    );
    expect(getAccessToken).toHaveBeenCalledWith({ forceRefresh: true });
  });

  it("uses AuthKit's ordinary cached-token flow by default", async () => {
    const getAccessToken = vi
      .fn<GetAccessToken>()
      .mockResolvedValue("cached-token");
    const fetchAccessToken = makeFetchAccessToken(getAccessToken);

    await expect(fetchAccessToken({ forceRefreshToken: false })).resolves.toBe(
      "cached-token",
    );
    expect(getAccessToken).toHaveBeenCalledWith();
  });
});
