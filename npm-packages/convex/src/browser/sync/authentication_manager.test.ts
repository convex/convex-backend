import { setImmediate } from "node:timers/promises";
import { BaseConvexClient } from "../index.js";
import { afterEach, describe, expect, test, vi } from "vitest";

type WireMessage = { type: string; tokenType?: string; value?: string };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

const clients: Array<BaseConvexClient> = [];

afterEach(async () => {
  await Promise.all(clients.splice(0).map((client) => client.close()));
});

function setup() {
  const messages: Array<WireMessage> = [];
  let socket!: TestSocket;
  class TestSocket {
    readyState = 0;
    onopen: (() => void) | null = null;
    onclose: ((event: { code: number; reason: string }) => void) | null = null;
    onerror: ((event: unknown) => void) | null = null;
    onmessage: ((event: { data: string }) => void) | null = null;

    constructor() {
      socket = this;
    }

    open() {
      this.readyState = 1;
      this.onopen?.();
    }

    send(data: string) {
      messages.push(JSON.parse(data) as WireMessage);
    }

    close() {
      this.readyState = 3;
      queueMicrotask(() =>
        this.onclose?.({ code: 1000, reason: "test close" }),
      );
    }
  }

  // Keep the real SDK auth manager, query queue and socket pause/resume logic.
  // Only the transport is simulated; no server grants or permissions change.
  const client = new BaseConvexClient(
    "https://auth-test.convex.cloud",
    () => {},
    {
      webSocketConstructor: TestSocket as unknown as typeof WebSocket,
      expectAuth: true,
      initialAuthTokenReuse: true,
      logger: false,
      unsavedChangesWarning: false,
    },
  );
  clients.push(client);
  client.subscribe("authProbe:identity", {});
  socket.open();
  expect(messages).toEqual([]);
  return { client, messages };
}

async function startInitialRefetch(client: BaseConvexClient) {
  const pending = deferred<string | null>();
  const fetchToken = vi
    .fn()
    .mockResolvedValueOnce(null)
    .mockImplementation(() => pending.promise);
  const onAuthChange = vi.fn();
  client.setAuth(fetchToken, onAuthChange);
  await vi.waitFor(() => expect(fetchToken).toHaveBeenCalledTimes(2));
  return { pending, fetchToken, onAuthChange };
}

describe("Convex initial auth refetch socket ownership", () => {
  test.each([null, "obsolete-token"])(
    "a synchronous replacement inside a refetch returning %s keeps the query queue paused",
    async (obsoleteToken) => {
      const fixture = setup();
      const replacement = deferred<string | null>();
      const oldOnAuthChange = vi.fn();
      const newOnAuthChange = vi.fn();
      fixture.client.setAuth(async ({ forceRefreshToken }) => {
        if (!forceRefreshToken) return null;
        fixture.client.setAuth(() => replacement.promise, newOnAuthChange);
        return obsoleteToken;
      }, oldOnAuthChange);
      await setImmediate();
      try {
        expect(fixture.messages).toEqual([]);
        expect(fixture.client.hasAuth()).toBe(false);
        expect(oldOnAuthChange).not.toHaveBeenCalled();
        expect(newOnAuthChange).not.toHaveBeenCalled();
      } finally {
        replacement.resolve("replacement-token");
        await setImmediate();
      }
      expect(
        fixture.messages.find((m) => m.type === "Authenticate"),
      ).toMatchObject({
        tokenType: "User",
        value: "replacement-token",
      });
    },
  );

  test("the current initial refetch sends authentication before the queued query", async () => {
    const fixture = setup();
    const current = await startInitialRefetch(fixture.client);
    expect(fixture.messages).toEqual([]);

    current.pending.resolve("fresh-token");
    await setImmediate();

    const authIndex = fixture.messages.findIndex(
      (m) => m.type === "Authenticate",
    );
    const queryIndex = fixture.messages.findIndex(
      (m) => m.type === "ModifyQuerySet",
    );
    expect(authIndex).toBeGreaterThanOrEqual(0);
    expect(queryIndex).toBeGreaterThan(authIndex);
    expect(fixture.messages[authIndex]).toMatchObject({
      tokenType: "User",
      value: "fresh-token",
    });
    expect(fixture.client.hasAuth()).toBe(true);
    expect(current.onAuthChange).not.toHaveBeenCalled();
  });

  test("a failed current refetch still reports signed out and releases the query queue", async () => {
    const fixture = setup();
    const onAuthChange = vi.fn();
    fixture.client.setAuth(async () => null, onAuthChange);
    await setImmediate();

    expect(onAuthChange).toHaveBeenCalledExactlyOnceWith(false);
    expect(fixture.client.hasAuth()).toBe(false);
    expect(fixture.messages.some((m) => m.type === "ModifyQuerySet")).toBe(
      true,
    );
  });

  test.each([null, "obsolete-token"])(
    "an obsolete refetch returning %s cannot release queries while the next identity is pending",
    async (obsoleteToken) => {
      const fixture = setup();
      const obsolete = await startInitialRefetch(fixture.client);
      const replacement = deferred<string | null>();
      const newOnAuthChange = vi.fn();
      fixture.client.setAuth(() => replacement.promise, newOnAuthChange);

      obsolete.pending.resolve(obsoleteToken);
      await setImmediate();
      try {
        expect([...fixture.messages]).toEqual([]);
        expect(fixture.client.hasAuth()).toBe(false);
        expect(obsolete.onAuthChange).not.toHaveBeenCalled();
        expect(newOnAuthChange).not.toHaveBeenCalled();
      } finally {
        replacement.resolve("replacement-token");
        await setImmediate();
      }

      const authIndex = fixture.messages.findIndex(
        (m) => m.type === "Authenticate",
      );
      const queryIndex = fixture.messages.findIndex(
        (m) => m.type === "ModifyQuerySet",
      );
      expect(authIndex).toBeGreaterThanOrEqual(0);
      expect(queryIndex).toBeGreaterThan(authIndex);
      expect(fixture.messages[authIndex]).toMatchObject({
        tokenType: "User",
        value: "replacement-token",
      });
      expect(fixture.client.hasAuth()).toBe(true);
    },
  );

  test("a late obsolete refetch cannot replace or clear a newer identity", async () => {
    const fixture = setup();
    const obsolete = await startInitialRefetch(fixture.client);
    fixture.client.setAuth(async () => "replacement-token", vi.fn());
    await setImmediate();
    const messagesBeforeObsolete = [...fixture.messages];

    obsolete.pending.resolve(null);
    await setImmediate();

    expect(fixture.messages).toEqual(messagesBeforeObsolete);
    expect(fixture.client.hasAuth()).toBe(true);
    expect(obsolete.onAuthChange).not.toHaveBeenCalled();
  });

  test("closing the client revokes a pending initial refetch", async () => {
    const fixture = setup();
    const pending = await startInitialRefetch(fixture.client);
    await fixture.client.close();
    pending.pending.resolve("obsolete-token");
    await setImmediate();

    expect(fixture.messages).toEqual([]);
    expect(pending.onAuthChange).not.toHaveBeenCalled();
  });
});
