---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: server
---

import Link from "@docusaurus/Link";

The Convex functions of the core, exported at `@convex-dev/auth/server`.

[setupConvexAuth](#setupconvexauth) builds the session functions that the app
exports. [convexAuthProvider](#convexauthprovider) builds the provider that
`convex/auth.config.ts` lists. [getAuthUserId](#getauthuserid) reads the user id
of the caller in a query, a mutation, or an action.

## Type Aliases

### AuthCore

```ts
type AuthCore<UsersTable> = object;
```

The core auth API returned by [setupConvexAuth](#setupconvexauth): the session
handlers the app re-exports. The sign-in methods also use it to bind to the
core.

#### Type Parameters

| Type Parameter                  | Default type |
| ------------------------------- | ------------ |
| `UsersTable` _extends_ `string` | `string`     |

#### Properties

| Property                                                 | Type                                                                                                                               | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| -------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| <Link id="property-isauthenticated" /> `isAuthenticated` | `RegisteredQuery`\<`"public"`, `Record`\<`string`, `never`\>, `Promise`\<`boolean`\>\>                                             | Reports whether the caller's access token identifies a live session. Convex verifies the token's signature (against the deployment's JWKS) before this query runs, so the result is a trustworthy authentication verdict. SSR hosts call it with the token from the auth cookie to decide authentication state, rather than trusting the (client-controlled) cookie by merely decoding it. The query reads the session, so it returns `false` once the session is gone (signed out elsewhere, revoked, or expired) even while the token itself is still valid, and a subscription re-runs when that happens. |
| <Link id="property-refreshsession" /> `refreshSession`   | `RegisteredMutation`\<`"public"`, \{ `refreshToken`: `string`; \}, `Promise`\<[`RefreshResult`](../ssr/index.md#refreshresult)\>\> | Refreshes a session using the given token, rotating the refresh token. Resolves to one of the [RefreshResult](../ssr/index.md#refreshresult) outcomes. `rotated` carries a new token bundle to persist; the presented token keeps working until the new refresh token is first used, so a lost response can be retried. A `noSession` result should be treated as signed-out.                                                                                                                                                                                                                                |
| <Link id="property-signout" /> `signOut`                 | `RegisteredMutation`\<`"public"`, \{ `refreshToken`: `string`; \}, `Promise`\<`null`\>\>                                           | Signs out of the current session. After this the refresh token is no longer valid.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                           |
| <Link id="property-userstable" /> `usersTable`           | `UsersTable`                                                                                                                       | The name of the app's users table, as configured on [setupConvexAuth](#setupconvexauth). Carried on the value (not just in the type) so method setup functions can infer `UsersTable` from the `core` they are handed, and so the name is available for error messages.                                                                                                                                                                                                                                                                                                                                      |

## Functions

### convexAuthProvider()

```ts
function convexAuthProvider(): AuthProvider;
```

The provider that the app lists in `convex/auth.config.ts` to accept the access
tokens of Convex Auth.

Access tokens are signed with the deployment's key: the auth component mints
them via `ctx.auth.signJwt`, so their issuer is the component's path under the
deployment, and the public keys are the deployment's JWKS.

```ts
// convex/auth.config.ts
import type { AuthConfig } from "convex/server";
import { convexAuthProvider } from "@convex-dev/auth/server";

export default {
  providers: [convexAuthProvider()],
} satisfies AuthConfig;
```

The app must mount the auth component as `auth` (`app.use(auth)` in
`convex.config.ts`), which is the default name.

#### Returns

`AuthProvider`

---

### getAuthUserId()

```ts
function getAuthUserId<UsersTable>(
  ctx,
  component?,
): Promise<Id<UsersTable> | null>;
```

The ID of the user the caller is signed in as, or `null` when the caller is not
signed in.

Call it with the function's `ctx` (query, mutation, or action).

```ts
import { getAuthUserId } from "@convex-dev/auth/server";

export const loggedInUser = query({
  args: {},
  handler: async (ctx) => {
    const userId = await getAuthUserId(ctx);
    if (userId === null) {
      return null;
    }
    return await ctx.db.get("users", userId);
  },
});
```

The JWT the client shares with the backend identifies a session, not a user:
this looks the session up, at the cost of one component call, and returns `null`
once it is gone (signed out, revoked, or expired) even while the token itself is
still valid. In a subscribed query, deleting the session re-runs the query.

The id is typed as one in the `"users"` table. If the app's users live in a
table with another name (see `setupConvexAuth`'s `usersTable` option), name it
here too:

```ts
const userId = await getAuthUserId<"members">(ctx);
```

By default this calls the core component mounted under its default name, `auth`.
If the app mounts it under another name, pass that component:

```ts
const userId = await getAuthUserId(ctx, components.myAuth);
```

#### Type Parameters

| Type Parameter                  | Default type |
| ------------------------------- | ------------ |
| `UsersTable` _extends_ `string` | `"users"`    |

#### Parameters

| Parameter      | Type                                                                                                               | Default value      |
| -------------- | ------------------------------------------------------------------------------------------------------------------ | ------------------ |
| `ctx`          | \{ `auth`: `Auth`; `runQuery`: \<`Query`\>(`query`, ...`args`) => `Promise`\<`FunctionReturnType`\<`Query`\>\>; \} | `undefined`        |
| `ctx.auth`     | `Auth`                                                                                                             | `undefined`        |
| `ctx.runQuery` | \<`Query`\>(`query`, ...`args`) => `Promise`\<`FunctionReturnType`\<`Query`\>\>                                    | `undefined`        |
| `component`    | `ComponentApi`                                                                                                     | `defaultComponent` |

#### Returns

`Promise`\<`Id`\<`UsersTable`\> \| `null`\>

---

### setupConvexAuth()

```ts
function setupConvexAuth<UsersTable>(options): AuthCore<UsersTable>;
```

Build the app-facing auth-core handlers from the mounted `core` component
reference. Returns ready-to-export `signOut`/`refreshSession`/ `isAuthenticated`
handlers. Give the result to the setup function of each method:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const { signUpWithUsernamePassword, signInWithUsernamePassword } =
  setupUsernamePassword(auth, {
    component: components.authPassword,
    usernameComponent: components.authUsername,
  }).attachUserCallbacks({ createUser: internal.users.createUserPassword });
```

The core never triggers a sign-in itself: it has no idea how any given method
authenticates a user. _Triggering sign-in is the responsibility of each method._
A method:

1. verifies the user its own way (checking a password, say),
2. looks the identity's app user up in its own component,
3. calls `completeSignUp` for a new identity, or `completeSignIn` for a known
   one, to get a session.

The core injects both helpers on `ctx.convexAuth`.

Token and session lifetimes are configurable here. The access-token TTL defaults
to 1m, and `sessionMaxLifetimeSeconds` to 30d. `refreshInactivityTimeoutSeconds`
defaults to the max lifetime and cannot exceed it. The access-token TTL must be
shorter than the inactivity timeout, and (since the client refreshes shortly
before expiry) comfortably longer than a few seconds.

Changes to access token TTLs impact newly minted tokens, not ones that have
already been issued. Session/refresh related TTLs apply to all issued tokens.

#### Type Parameters

| Type Parameter                  | Default type |
| ------------------------------- | ------------ |
| `UsersTable` _extends_ `string` | `"users"`    |

#### Parameters

| Parameter                                  | Type                                                                                                                                                                                      | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                  |
| ------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `options`                                  | \{ `accessTokenTtlSeconds?`: `number`; `component`: `ComponentApi`; `refreshInactivityTimeoutSeconds?`: `number`; `sessionMaxLifetimeSeconds?`: `number`; `usersTable?`: `UsersTable`; \} | -                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `options.accessTokenTtlSeconds?`           | `number`                                                                                                                                                                                  | Access-token lifetime in seconds. Defaults to 60 (1 minute). Changes to this value impact newly minted tokens, not ones that have already been issued.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |
| `options.component`                        | `ComponentApi`                                                                                                                                                                            | -                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| `options.refreshInactivityTimeoutSeconds?` | `number`                                                                                                                                                                                  | How long in seconds a session may go without refreshing before it expires. Cannot exceed `sessionMaxLifetimeSeconds`. A connected client refreshes about once per access-token TTL, so this is how long a client may stay away (closed, offline, asleep) and still resume its session. Changes to this value apply to every session, including ones that already exist: a session expires this long after its last refresh, under the value configured when it is checked. Lowering it thus ends sessions that have gone longer than the new value without refreshing.                                                                                                       |
| `options.sessionMaxLifetimeSeconds?`       | `number`                                                                                                                                                                                  | How long in seconds a session may last. Defaults to 30 days. Once it is reached, the user has to sign in again. A session expires at the sooner of this and its refresh inactivity timeout (`refreshInactivityTimeoutSeconds`). Changes to this value apply to every session, including ones that already exist: a session expires this long after it was created, under the value configured when it is checked.                                                                                                                                                                                                                                                            |
| `options.usersTable?`                      | `UsersTable`                                                                                                                                                                              | The name of the app's users table. Defaults to `"users"`. The core never reads or writes the table — it only stores the id the app's `createUser` callback returns. The name is a _type-level_ contract: it makes the `attachUserCallbacks` of every method demand a `createUser` returning `Id<usersTable>` and an `onSignIn` taking one, so an app cannot accidentally hand back an id from some other table (and, because the app then declares `v.id(usersTable)`, Convex enforces the same thing at runtime). Set this if the app's users live in a table with a different name: `const auth = setupConvexAuth({ component: components.auth, usersTable: "members" });` |

#### Returns

[`AuthCore`](#authcore)\<`UsersTable`\>
