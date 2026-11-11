import { describe, expect, test } from "vitest";
import {
  GlobalConfig,
  findInstanceForDirectory,
  resolveAccessToken,
  withAccount,
  withInstance,
  withDefaultAccount,
  withRekeyedAccount,
  shouldRevokeReplacedTokens,
  withSetting,
  withoutAccount,
} from "./globalConfig.js";

const twoAccounts: GlobalConfig = {
  accessToken: "token-a",
  accounts: {
    a: { accessToken: "token-a", isDefault: true },
    b: { accessToken: "token-b", description: "Client B" },
  },
  instances: {
    "/work/client-b": { account: "b" },
    "/work/client-b/packages/admin": { account: "a" },
  },
};

describe("resolveAccessToken", () => {
  test("single-token config ignores the directory", () => {
    expect(resolveAccessToken({ accessToken: "legacy" }, "/anywhere")).toEqual({
      accessToken: "legacy",
      accountId: null,
      source: "legacy",
    });
  });

  test("bound directory and its subdirectories use the bound account", () => {
    expect(resolveAccessToken(twoAccounts, "/work/client-b")).toMatchObject({
      accessToken: "token-b",
      accountId: "b",
      source: "directory",
    });
    expect(
      resolveAccessToken(twoAccounts, "/work/client-b/convex"),
    ).toMatchObject({ accountId: "b" });
  });

  test("the closest bound ancestor wins", () => {
    expect(
      resolveAccessToken(twoAccounts, "/work/client-b/packages/admin/src"),
    ).toMatchObject({ accountId: "a", source: "directory" });
  });

  test("a sibling with a shared name prefix is not inside the bound directory", () => {
    expect(
      findInstanceForDirectory(twoAccounts, "/work/client-b-old"),
    ).toBeNull();
    expect(resolveAccessToken(twoAccounts, "/work/client-b-old")).toMatchObject(
      { accountId: "a", source: "default" },
    );
  });

  test("a directory bound to a missing account does not fall back", () => {
    const config: GlobalConfig = {
      ...twoAccounts,
      instances: { "/work/gone": { account: "gone" } },
    };
    expect(resolveAccessToken(config, "/work/gone")).toBeNull();
  });
});

describe("withAccount", () => {
  test("the first named account keeps the existing token as the default `unidentified` account", () => {
    const config = withAccount({ accessToken: "old" }, "b", {
      accessToken: "token-b",
    });
    expect(config).toEqual({
      accessToken: "old",
      accounts: {
        unidentified: { accessToken: "old", isDefault: true },
        b: { accessToken: "token-b" },
      },
      instances: {},
    });
  });

  test("the first account on a fresh machine becomes the default and is mirrored", () => {
    const config = withAccount(null, "a", {
      accessToken: "token-a",
      description: "Work",
    });
    expect(config.accessToken).toBe("token-a");
    expect(config.accounts?.a).toEqual({
      accessToken: "token-a",
      description: "Work",
      isDefault: true,
    });
  });

  test("refreshing the default account's token updates the mirror", () => {
    const config = withAccount(twoAccounts, "a", { accessToken: "new-a" });
    expect(config.accessToken).toBe("new-a");
    expect(config.accounts?.b?.description).toBe("Client B");
  });
});

describe("withoutAccount", () => {
  test("removing the default promotes another and drops its bindings", () => {
    const config = withoutAccount(twoAccounts, "a");
    expect(config.accessToken).toBe("token-b");
    expect(config.accounts).toEqual({
      b: { accessToken: "token-b", description: "Client B", isDefault: true },
    });
    expect(config.instances).toEqual({ "/work/client-b": { account: "b" } });
  });

  test("removing the last account removes the top-level token", () => {
    const config = withoutAccount(withoutAccount(twoAccounts, "a"), "b");
    expect(config.accessToken).toBeUndefined();
    expect(config.accounts).toEqual({});
  });
});

test("withDefaultAccount moves the flag and the mirror", () => {
  const config = withDefaultAccount(twoAccounts, "b");
  expect(config.accessToken).toBe("token-b");
  expect(config.accounts?.a?.isDefault).toBeUndefined();
  expect(config.accounts?.b?.isDefault).toBe(true);
});

test("withInstance binds and unbinds a directory", () => {
  const bound = withInstance(twoAccounts, "/work/new", "b");
  expect(bound.instances?.["/work/new"]).toEqual({ account: "b" });
  const unbound = withInstance(bound, "/work/new", null);
  expect(unbound.instances?.["/work/new"]).toBeUndefined();
});

test("withRekeyedAccount moves `unidentified` to its member id with its directories", () => {
  const config = withRekeyedAccount(twoAccounts, "a", "230405");
  expect(config.accounts?.["230405"]).toEqual({
    accessToken: "token-a",
    isDefault: true,
  });
  expect(config.accounts?.a).toBeUndefined();
  expect(config.accessToken).toBe("token-a");
  expect(config.instances?.["/work/client-b/packages/admin"]).toEqual({
    account: "230405",
  });
});

test("withRekeyedAccount onto a saved member keeps the member's own entry", () => {
  const config = withRekeyedAccount(twoAccounts, "a", "b");
  expect(config.accounts).toEqual({
    b: { accessToken: "token-b", description: "Client B", isDefault: true },
  });
  expect(config.instances?.["/work/client-b/packages/admin"]).toEqual({
    account: "b",
  });
});

test("withAccount records the email an account belongs to", () => {
  const config = withAccount(twoAccounts, "b", {
    accessToken: "new-b",
    email: "b@example.com",
  });
  expect(config.accounts?.b).toMatchObject({
    accessToken: "new-b",
    email: "b@example.com",
    description: "Client B",
  });
});

test("revoking replaced tokens is on unless turned off", () => {
  expect(shouldRevokeReplacedTokens(null)).toBe(true);
  expect(shouldRevokeReplacedTokens(twoAccounts)).toBe(true);
  const off = withSetting(twoAccounts, "revokeReplacedTokens", false);
  expect(shouldRevokeReplacedTokens(off)).toBe(false);
  expect(off.accounts).toEqual(twoAccounts.accounts);
});
