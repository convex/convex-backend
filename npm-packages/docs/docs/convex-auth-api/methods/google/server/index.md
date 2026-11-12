---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/google/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

The `google` sign-in method, exported at
`@convex-dev/auth/methods/google/server`.

## Type Aliases

### GoogleOptions

```ts
type GoogleOptions = object;
```

App-defined config for setting up the `google` method.

#### Properties

| Property                                                               | Type           | Description                                                                                         |
| ---------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedredirectorigins" /> `allowedRedirectOrigins` | `string`[]     | Origins `redirectTo` may point at, e.g. `["https://app.example.com"]` for open-redirect prevention. |
| <Link id="property-component" /> `component`                           | `ComponentApi` | The Google oauth component instance, i.e. `components.authGoogle`.                                  |

---

### GoogleProfile

```ts
type GoogleProfile = Infer<typeof vGoogleProfile>;
```

## Variables

### vGoogleProfile

```ts
const vGoogleProfile: VObject<
  {
    email?: string;
    emailVerified: boolean;
    id: string;
    name?: string;
    picture?: string;
  },
  {
    email: VString<string | undefined, "optional">;
    emailVerified: VBoolean<boolean, "required">;
    id: VString<string, "required">;
    name: VString<string | undefined, "optional">;
    picture: VString<string | undefined, "optional">;
  },
  "required",
  "id" | "email" | "name" | "emailVerified" | "picture"
>;
```

The account profile that the `google` method produces. Google is OIDC, so
identity comes from validated id_token claims. This is the exact shape the
mapping emits, so the app can validate against it precisely.

## Functions

### normalizeGoogleProfile()

```ts
function normalizeGoogleProfile(claims): object;
```

Map validated Google id_token claims to [GoogleProfile](#googleprofile). Google
returns an id_token, so `claims` is always present here. A missing one is a bug.

#### Parameters

| Parameter | Type                        |
| --------- | --------------------------- |
| `claims`  | `OidcClaims` \| `undefined` |

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

##### picture?

```ts
optional picture?: string;
```

---

### setupGoogle()

```ts
function setupGoogle<UsersTable>(core, options): object;
```

Built-in sign-in method for Google. Wire it up with the Google oauth component:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const { startSignInWithGoogle, completeSignInWithGoogle } = setupGoogle(
  auth,
  {
    component: components.authGoogle,
    allowedRedirectOrigins: [
      "https://app.example.com",
      "http://localhost:5173",
    ],
  },
).attachUserCallbacks({ createUser: internal.users.createUserGoogle });
```

Setup:

1. Install the component in convex.config.ts under
   `httpPrefix: "/oauth/google"`, binding Google's `CLIENT_ID` and
   `CLIENT_SECRET`.
2. Register `<site-url>/oauth/google/callback` as the redirect URI with Google.

The `httpPrefix` alone determines the callback URL.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`GoogleOptions`](#googleoptions)                               |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(callbacks): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter   | Type                                                                                                                                                        |
| ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `callbacks` | `UserCallbacks`\<`"google"`, \{ `email?`: `string`; `emailVerified`: `boolean`; `id`: `string`; `name?`: `string`; `picture?`: `string`; \}, `UsersTable`\> |

###### Returns

`object`

###### completeSignInWithGoogle

```ts
completeSignInWithGoogle: RegisteredMutation<
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

###### startSignInWithGoogle

```ts
startSignInWithGoogle: RegisteredMutation<
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
