---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: react
---

import Link from "@docusaurus/Link";

React bindings for Convex Auth.

Wrap your app in [ConvexAuthProvider](#convexauthprovider) (in place of
`ConvexProvider`) to enable authentication. The provider owns the token
lifecycle — storing the session, refreshing the access token, signing out — and
feeds Convex's `ConvexProviderWithAuth` a `useAuth` hook.

Sign-in methods are deliberately not part of this core: each sign-in method
(password, OAuth, passkey, …) ships its own authentication API that returns a
[TokenBundle](#tokenbundle). Hand that bundle to
[useAuthActions](#useauthactions)'s `setSession` which allows the Convex client
to authenticate with the backend.

## Interfaces

- [TokenStorage](interfaces/TokenStorage.md)

## Type Aliases

### ConvexAuthActionsContextType

```ts
type ConvexAuthActionsContextType = object;
```

The value exposed by [useAuthActions](#useauthactions).

#### Properties

| Property                                       | Type                               | Description                                                                                                                                                                                                                                                                                                                                                     |
| ---------------------------------------------- | ---------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-setsession" /> `setSession` | (`session`) => `Promise`\<`void`\> | Initialize or refresh a session. A SPA client will receive a full [TokenBundle](#tokenbundle) so it can access the refresh token for direct session refreshing. An SSR client will receive a [SlimTokenBundle](../ssr/index.md#slimtokenbundle) which doesn't include the refresh token (that value is in an httpOnly cookie, not reachable by client JS code). |
| <Link id="property-signout" /> `signOut`       | () => `Promise`\<`void`\>          | Sign out: revoke the session on the server and clear it locally.                                                                                                                                                                                                                                                                                                |

---

### ConvexAuthApi

```ts
type ConvexAuthApi = object;
```

The auth functions the app exports from `setupConvexAuth`. Passed as references
(not names) because an app may re-export them under any names. Consumed by both
SPA and SSR implementations.

#### Properties

| Property                                                 | Type                                                     |
| -------------------------------------------------------- | -------------------------------------------------------- |
| <Link id="property-isauthenticated" /> `isAuthenticated` | [`IsAuthenticatedFn`](../ssr/index.md#isauthenticatedfn) |
| <Link id="property-refreshsession" /> `refreshSession`   | [`RefreshSessionFn`](../ssr/index.md#refreshsessionfn)   |
| <Link id="property-signout-1" /> `signOut`               | [`SignOutFn`](../ssr/index.md#signoutfn)                 |

---

### TokenBundle

```ts
type TokenBundle = Infer<typeof vTokenBundle>;
```

## Functions

### ConvexAuthProvider()

```ts
function ConvexAuthProvider(__namedParameters): Element;
```

Replace your `ConvexProvider` with this to enable authentication.

```tsx
import { ConvexAuthProvider } from "@convex-dev/auth/react";
import { ConvexReactClient } from "convex/react";
import { api } from "../convex/_generated/api";

const convex = new ConvexReactClient(import.meta.env.VITE_CONVEX_URL);

function Root({ children }: { children: React.ReactNode }) {
  return (
    <ConvexAuthProvider
      client={convex}
      api={{
        refreshSession: api.auth.refreshSession,
        signOut: api.auth.signOut,
        isAuthenticated: api.auth.isAuthenticated,
      }}
    >
      {children}
    </ConvexAuthProvider>
  );
}
```

#### Parameters

| Parameter                             | Type                                                                                                                                                                                                                                         | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                            |
| ------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `__namedParameters`                   | \{ `ambientSignIns?`: `AmbientSignInClient`[]; `api`: [`ConvexAuthApi`](#convexauthapi); `children`: `ReactNode`; `client`: `ConvexReactClient`; `storage?`: [`TokenStorage`](interfaces/TokenStorage.md); `storageNamespace?`: `string`; \} | -                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `__namedParameters.ambientSignIns?`   | `AmbientSignInClient`[]                                                                                                                                                                                                                      | Advanced. Ambient sign-ins run initialization for sign-in methods that can take action outside of a user activated sign in flow, such as reading an oauth code from a url query param. Setting this replaces the default (`[oauth()]`) entirely rather than adding to it. Pass `[]` to register nothing, or include `oauth()` (from `@convex-dev/auth/methods/oauth/react`) yourself to keep it alongside other sign-ins. Read once when the client is created and not expected to change.                                                                                                                                                                                             |
| `__namedParameters.api`               | [`ConvexAuthApi`](#convexauthapi)                                                                                                                                                                                                            | The app's `refreshSession` and `signOut` mutation references, and its `isAuthenticated` query reference. The provider subscribes to `isAuthenticated`, so a session revoked server-side signs the client out reactively.                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `__namedParameters.children`          | `ReactNode`                                                                                                                                                                                                                                  | -                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                      |
| `__namedParameters.client`            | `ConvexReactClient`                                                                                                                                                                                                                          | Your [`ConvexReactClient`](https://docs.convex.dev/api/classes/react.ConvexReactClient).                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                               |
| `__namedParameters.storage?`          | [`TokenStorage`](interfaces/TokenStorage.md)                                                                                                                                                                                                 | A custom [TokenStorage](interfaces/TokenStorage.md) implementation. If none is supplied, the system defaults to `localStorage` in the browser and in-memory where there is no `localStorage` (SSR). Client runtimes with no `localStorage` like React Native are strongly advised to provide an implementation, because the in-memory default will cause users to get logged out each time the app closes. Here's an example of an implementation for Expo that could be passed in here: `import * as SecureStore from "expo-secure-store"; const secureStorage = { getItem: SecureStore.getItemAsync, setItem: SecureStore.setItemAsync, removeItem: SecureStore.deleteItemAsync, };` |
| `__namedParameters.storageNamespace?` | `string`                                                                                                                                                                                                                                     | Namespace for storage keys, which determines whether tokens are shared. Non-alphanumeric characters are ignored. Defaults to the deployment URL.                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

#### Returns

`Element`

---

### useAuthActions()

```ts
function useAuthActions(): ConvexAuthActionsContextType;
```

Access the auth actions:

```ts
const { setSession, signOut } = useAuthActions();
```

- `setSession` adopts a [TokenBundle](#tokenbundle) produced by the sign-in of a
  method.
- `signOut` revokes and clears the session.

#### Returns

[`ConvexAuthActionsContextType`](#convexauthactionscontexttype)

---

### useAuthToken()

```ts
function useAuthToken(): string | null;
```

The current access token (a JWT), or null when signed out.

Use it to authenticate requests to your Convex HTTP actions
(`Authorization: Bearer <token>`). Treat it as an ID token — do not send it to
other servers.

#### Returns

`string` \| `null`
