---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/apple/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

The `apple` sign-in method, exported at `@convex-dev/auth/methods/apple/server`.

## Type Aliases

### AppleOptions

```ts
type AppleOptions = object;
```

App-defined config for setting up the `apple` method.

#### Properties

| Property                                                               | Type           | Description                                                                                         |
| ---------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedredirectorigins" /> `allowedRedirectOrigins` | `string`[]     | Origins `redirectTo` may point at, e.g. `["https://app.example.com"]` for open-redirect prevention. |
| <Link id="property-component" /> `component`                           | `ComponentApi` | The Apple oauth component instance, i.e. `components.authApple`.                                    |

---

### AppleProfile

```ts
type AppleProfile = Infer<typeof vAppleProfile>;
```

---

### SanitizedAppleUser

```ts
type SanitizedAppleUser = object;
```

Only the name parts are returned after sanitizing.

#### Properties

| Property                           | Type     |
| ---------------------------------- | -------- |
| <Link id="property-name" /> `name` | `object` |
| `name.firstName?`                  | `string` |
| `name.lastName?`                   | `string` |

## Variables

### vAppleProfile

```ts
const vAppleProfile: VObject<
  {
    email?: string;
    emailVerified: boolean;
    id: string;
    name?: string;
  },
  {
    email: VString<string | undefined, "optional">;
    emailVerified: VBoolean<boolean, "required">;
    id: VString<string, "required">;
    name: VString<string | undefined, "optional">;
  },
  "required",
  "id" | "email" | "name" | "emailVerified"
>;
```

The account profile that the `apple` method produces. Apple is OIDC, so identity
comes from validated id_token claims, with the name (when there is one) coming
from the callback instead. This is the exact shape the mapping emits, so the app
can validate against it precisely.

## Functions

### normalizeAppleProfile()

```ts
function normalizeAppleProfile(claims, user): object;
```

Map Apple's validated id_token claims, plus the sanitized name from a first
authorization, to [AppleProfile](#appleprofile).

`user` is undefined on every sign-in after the first, which is why the profile's
`name` is optional. An app that wants to keep the name should store it when it
arrives and leave the stored one alone when it doesn't.

#### Parameters

| Parameter | Type                                                       |
| --------- | ---------------------------------------------------------- |
| `claims`  | `OidcClaims`                                               |
| `user`    | [`SanitizedAppleUser`](#sanitizedappleuser) \| `undefined` |

#### Returns

`object`

##### email?

```ts
optional email?: string;
```

##### emailVerified

```ts
emailVerified: boolean;
```

##### id

```ts
id: string;
```

##### name?

```ts
optional name?: string;
```

---

### setupApple()

```ts
function setupApple<UsersTable>(core, options): object;
```

Built-in sign-in method for Apple ("Sign in with Apple"). Wire it up with the
Apple oauth component:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const { startSignInWithApple, completeSignInWithApple } = setupApple(
  auth,
  {
    component: components.authApple,
    allowedRedirectOrigins: [
      "https://app.example.com",
      "http://localhost:5173",
    ],
  },
).attachUserCallbacks({ createUser: internal.users.createUserApple });
```

Setup:

1. Install the component in convex.config.ts under `httpPrefix: "/oauth/apple"`,
   binding the Services ID, team id, key id, and `.p8` private key.
2. Register `<site-url>/oauth/apple/callback` as a return URL on the Services
   ID, along with the `convex.site` domain it is on.

The `httpPrefix` alone determines the callback URL, on the deployment's
`convex.site` host. Apple has to accept that host as a return URL and rejects
`localhost` and IP addresses, so the deployment has to be publicly reachable.
The app the flow returns to is unaffected and can stay on localhost.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`AppleOptions`](#appleoptions)                                 |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(callbacks): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter   | Type                                                                                                                                 |
| ----------- | ------------------------------------------------------------------------------------------------------------------------------------ |
| `callbacks` | `UserCallbacks`\<`"apple"`, \{ `email?`: `string`; `emailVerified`: `boolean`; `id`: `string`; `name?`: `string`; \}, `UsersTable`\> |

###### Returns

`object`

###### completeSignInWithApple

```ts
completeSignInWithApple: RegisteredMutation<
  "public",
  {
    code: string;
    state: string;
  },
  Promise<
    | {
        status: "complete";
        tokens: {
          accessToken: string;
          refreshToken: string;
          refreshTokenExpiresAt: number;
          userId: string;
        };
      }
    | {
        status: "error";
        userError: {
          error: "INVALID_CODE";
        };
      }
  >
> = completeSignIn;
```

###### startSignInWithApple

```ts
startSignInWithApple: RegisteredMutation<
  "public",
  {
    redirectTo: string;
  },
  Promise<{
    redirect: string;
    state: string;
  }>
> = startSignIn;
```
