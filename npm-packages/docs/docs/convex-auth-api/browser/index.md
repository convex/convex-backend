---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: browser
---

Framework-agnostic browser building blocks for Convex Auth clients: a token
storage abstraction, a cross-tab refresh mutex, and the session manager that
ties them together. The React bindings (`@convex-dev/auth/react`) build on
these, and other client libraries can too.

## Classes

- [AuthClient](classes/AuthClient.md)
- [InMemoryStorage](classes/InMemoryStorage.md)

## Interfaces

- [AuthState](interfaces/AuthState.md)
- [SpaAuthApi](interfaces/SpaAuthApi.md)
- [SsrAuthApi](interfaces/SsrAuthApi.md)

## Type Aliases

### AuthClientConfig

```ts
type AuthClientConfig =
  | (AuthClientConfigBase & object)
  | (AuthClientConfigBase & object);
```

Configuration for the [AuthClient](classes/AuthClient.md), discriminated by
session `mode`.

The mode determines who holds the refresh token, and therefore the shape of the
auth API the client drives:

- `"spa"`: JS holds the refresh token; the client passes it to a
  [SpaAuthApi](interfaces/SpaAuthApi.md).
- `"ssr"`: the refresh token is in an httpOnly cookie; a
  [SsrAuthApi](interfaces/SsrAuthApi.md) is called without one and reads the
  cookie server-side.

## Variables

### JWT_STORAGE_KEY

```ts
const JWT_STORAGE_KEY: "__convexAuthJWT" = "__convexAuthJWT";
```

Storage key for the current access token (a JWT).

---

### REFRESH_TOKEN_STORAGE_KEY

```ts
const REFRESH_TOKEN_STORAGE_KEY: "__convexAuthRefreshToken" =
  "__convexAuthRefreshToken";
```

Storage key for the current (rotating) refresh token.

## Functions

### defaultStorage()

```ts
function defaultStorage(): TokenStorage;
```

The default storage: the browser's `localStorage` when available, otherwise an
[InMemoryStorage](classes/InMemoryStorage.md). Callers that need cross-tab
sessions, React Native, or per-tab sessions should pass their own storage
instead.

#### Returns

[`TokenStorage`](../react/interfaces/TokenStorage.md)

---

### runWithMutex()

```ts
function runWithMutex<T>(key, callback): Promise<T>;
```

Run `callback` while holding the lock named `key`, resolving to its result. Only
one callback per key runs at a time.

#### Type Parameters

| Type Parameter |
| -------------- |
| `T`            |

#### Parameters

| Parameter  | Type                   |
| ---------- | ---------------------- |
| `key`      | `string`               |
| `callback` | () => `Promise`\<`T`\> |

#### Returns

`Promise`\<`T`\>

## References

### TokenStorage

Re-exports [TokenStorage](../react/interfaces/TokenStorage.md)
