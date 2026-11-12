---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: AuthClient
---

Framework-agnostic owner of the auth session on the client.

It persists the [TokenBundle](../../react/index.md#tokenbundle) from a sign-in,
hands out the access token via [AuthClient.fetchAccessToken](#fetchaccesstoken)
(refreshing under a cross-tab lock when forced), keeps in sync across tabs, and
exposes a subscribe/snapshot API so any UI framework can render its state. It
establishes sessions from the authentication results of the sign-in methods.
Methods authenticate and sign-in users and call
[AuthClient.setSession](#setsession) with a token bundle.

## Constructors

### Constructor

```ts
new AuthClient(config): AuthClient;
```

#### Parameters

| Parameter | Type                                               |
| --------- | -------------------------------------------------- |
| `config`  | [`AuthClientConfig`](../index.md#authclientconfig) |

#### Returns

`AuthClient`

## Methods

### ambientSignInValues()

```ts
ambientSignInValues(id): SignInValuesReader;
```

A read-only view of the values published under an ambient sign-in's id, for
hooks and other bindings to read and subscribe to. Only the sign-in itself can
write.

#### Parameters

| Parameter | Type     |
| --------- | -------- |
| `id`      | `string` |

#### Returns

`SignInValuesReader`

---

### dispose()

```ts
dispose(): void;
```

Detach a registered cross-tab storage listener, if applicable.

Call on teardown. Store subscribers are intentionally left in place — each is
removed via the unsubscribe returned by [subscribe](#subscribe) — so a later
[init](#init) restores the client without losing its subscribers.

#### Returns

`void`

---

### fetchAccessToken()

```ts
fetchAccessToken(__namedParameters): Promise<string | null>;
```

The function handed to Convex's `ConvexProviderWithAuth`.

Returns a cached access token, or fetches a new token if `forceRefreshToken` is
`true`.

#### Parameters

| Parameter                             | Type                                  |
| ------------------------------------- | ------------------------------------- |
| `__namedParameters`                   | \{ `forceRefreshToken`: `boolean`; \} |
| `__namedParameters.forceRefreshToken` | `boolean`                             |

#### Returns

`Promise`\<`string` \| `null`\>

---

### getAccessToken()

```ts
getAccessToken(): string | null;
```

The current access token, or null.

#### Returns

`string` \| `null`

---

### getSnapshot()

```ts
getSnapshot(): AuthState;
```

Returns the latest [AuthState](../interfaces/AuthState.md) snapshot.

#### Returns

[`AuthState`](../interfaces/AuthState.md)

---

### init()

```ts
init(): Promise<void>;
```

Run ambient sign-in onInit callbacks, load any persisted session from storage,
and start listening for cross-tab changes (if applicable). Until this resolves,
the client reports `isLoading`.

Symmetric and repeatable with [dispose](#dispose). The callbacks and session
load happen once, but the cross-tab listener is (re)attached on every call, so
an `init` after a `dispose` fully restores the client.

#### Returns

`Promise`\<`void`\>

---

### setSession()

```ts
setSession(session): Promise<void>;
```

Adopt the session a method just established. Methods call this after their own
sign-in flow returns a session: a full
[TokenBundle](../../react/index.md#tokenbundle) when the client holds the
refresh token (SPA), or an access-only SlimTokenBundle when the refresh token is
in a cookie (SSR).

#### Parameters

| Parameter | Type                                                                                                                                                                          |
| --------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `session` | \| \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \} \| [`SlimTokenBundle`](../../ssr/index.md#slimtokenbundle) |

#### Returns

`Promise`\<`void`\>

---

### signOut()

```ts
signOut(): Promise<void>;
```

Revoke the current session on the server and clear it locally.

#### Returns

`Promise`\<`void`\>

---

### subscribe()

```ts
subscribe(listener): () => void;
```

Subscribe to be notified when the [AuthState](../interfaces/AuthState.md)
changes.

Subscribers should call [getSnapshot](#getsnapshot) when notified of a change.

#### Parameters

| Parameter  | Type       |
| ---------- | ---------- |
| `listener` | `Listener` |

#### Returns

() => `void`

---

### withSignInPending()

```ts
withSignInPending<T>(fn): Promise<T>;
```

Run a sign-in completion while reporting `isLoading`. Use it for work that
establishes a session without a user action in the current page, like redeeming
an OAuth callback code after a redirect, so the UI shows a loading state instead
of flashing unauthenticated. The wrapped function should include its
[setSession](#setsession) call, so the loading state holds until the client is
authenticated.

#### Type Parameters

| Type Parameter |
| -------------- |
| `T`            |

#### Parameters

| Parameter | Type                   |
| --------- | ---------------------- |
| `fn`      | () => `Promise`\<`T`\> |

#### Returns

`Promise`\<`T`\>
