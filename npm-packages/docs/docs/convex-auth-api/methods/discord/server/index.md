---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/discord/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

The `discord` sign-in method, exported at
`@convex-dev/auth/methods/discord/server`.

## Type Aliases

### DiscordOptions

```ts
type DiscordOptions = object;
```

App-defined config for setting up the `discord` method.

#### Properties

| Property                                                               | Type           | Description                                                                                         |
| ---------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedredirectorigins" /> `allowedRedirectOrigins` | `string`[]     | Origins `redirectTo` may point at, e.g. `["https://app.example.com"]` for open-redirect prevention. |
| <Link id="property-component" /> `component`                           | `ComponentApi` | The Discord oauth component instance, i.e. `components.authDiscord`.                                |

---

### DiscordProfile

```ts
type DiscordProfile = Infer<typeof vDiscordProfile>;
```

---

### DiscordUserInfo

```ts
type DiscordUserInfo = object;
```

The responses the component's userinfo endpoints produce, keyed the way
USER_INFO_ENDPOINTS names them. This is what Discord is trusted to return; the
responses are not validated against it at runtime.

#### Properties

| Property                           | Type          |
| ---------------------------------- | ------------- |
| <Link id="property-user" /> `user` | `DiscordUser` |

## Variables

### vDiscordProfile

```ts
const vDiscordProfile: VObject<
  {
    avatarUrl: string;
    email?: string;
    emailVerified: boolean;
    id: string;
    name: string;
    username: string;
  },
  {
    avatarUrl: VString<string, "required">;
    email: VString<string | undefined, "optional">;
    emailVerified: VBoolean<boolean, "required">;
    id: VString<string, "required">;
    name: VString<string, "required">;
    username: VString<string, "required">;
  },
  "required",
  "id" | "username" | "email" | "name" | "emailVerified" | "avatarUrl"
>;
```

The account profile that the `discord` method produces. Discord is plain OAuth
(no id_token), so identity comes from its userinfo endpoint. This is the exact
shape the mapping emits, so the app can validate against it precisely.

## Functions

### normalizeDiscordProfile()

```ts
function normalizeDiscordProfile(userInfoResponses): object;
```

Map Discord's userinfo response to [DiscordProfile](#discordprofile).

#### Parameters

| Parameter           | Type                                                 |
| ------------------- | ---------------------------------------------------- |
| `userInfoResponses` | [`DiscordUserInfo`](#discorduserinfo) \| `undefined` |

#### Returns

`object`

##### avatarUrl

```ts
avatarUrl: string;
```

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

##### name

```ts
name: string;
```

##### username

```ts
username: string;
```

---

### setupDiscord()

```ts
function setupDiscord<UsersTable>(core, options): object;
```

Built-in sign-in method for Discord. Wire it up with the Discord oauth
component:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const { startSignInWithDiscord, completeSignInWithDiscord } =
  setupDiscord(auth, {
    component: components.authDiscord,
    allowedRedirectOrigins: [
      "https://app.example.com",
      "http://localhost:5173",
    ],
  }).attachUserCallbacks({ createUser: internal.users.createUserDiscord });
```

Setup:

1. Install the component in convex.config.ts under
   `httpPrefix: "/oauth/discord"`, binding Discord's `CLIENT_ID` and
   `CLIENT_SECRET`.
2. Register `<site-url>/oauth/discord/callback` as a redirect URI of the
   application in the Discord Developer Portal.

The `httpPrefix` alone determines the callback URL.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`DiscordOptions`](#discordoptions)                             |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(callbacks): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter   | Type                                                                                                                                                                               |
| ----------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `callbacks` | `UserCallbacks`\<`"discord"`, \{ `avatarUrl`: `string`; `email?`: `string`; `emailVerified`: `boolean`; `id`: `string`; `name`: `string`; `username`: `string`; \}, `UsersTable`\> |

###### Returns

`object`

###### completeSignInWithDiscord

```ts
completeSignInWithDiscord: RegisteredMutation<
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

###### startSignInWithDiscord

```ts
startSignInWithDiscord: RegisteredMutation<
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
