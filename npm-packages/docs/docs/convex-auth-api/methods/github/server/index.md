---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/github/server
sidebar_label: server
---

import Link from "@docusaurus/Link";

The `github` sign-in method, exported at
`@convex-dev/auth/methods/github/server`.

## Type Aliases

### GithubOptions

```ts
type GithubOptions = object;
```

App-defined config for setting up the `github` method.

#### Properties

| Property                                                               | Type           | Description                                                                                         |
| ---------------------------------------------------------------------- | -------------- | --------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedredirectorigins" /> `allowedRedirectOrigins` | `string`[]     | Origins `redirectTo` may point at, e.g. `["https://app.example.com"]` for open-redirect prevention. |
| <Link id="property-component" /> `component`                           | `ComponentApi` | The GitHub oauth component instance, i.e. `components.authGithub`.                                  |

---

### GithubProfile

```ts
type GithubProfile = Infer<typeof vGithubProfile>;
```

---

### GithubUserInfo

```ts
type GithubUserInfo = object;
```

The responses the component's userinfo endpoints produce, keyed the way
USER_INFO_ENDPOINTS names them. This is what GitHub is trusted to return; the
responses are not validated against it at runtime.

#### Properties

| Property                               | Type            |
| -------------------------------------- | --------------- |
| <Link id="property-emails" /> `emails` | `GithubEmail`[] |
| <Link id="property-user" /> `user`     | `GithubUser`    |

## Variables

### vGithubProfile

```ts
const vGithubProfile: VObject<
  {
    avatarUrl?: string;
    email?: string;
    emailVerified: boolean;
    id: string;
    login: string;
    name?: string;
  },
  {
    avatarUrl: VString<string | undefined, "optional">;
    email: VString<string | undefined, "optional">;
    emailVerified: VBoolean<boolean, "required">;
    id: VString<string, "required">;
    login: VString<string, "required">;
    name: VString<string | undefined, "optional">;
  },
  "required",
  "id" | "email" | "name" | "emailVerified" | "login" | "avatarUrl"
>;
```

The account profile that the `github` method produces. GitHub is plain OAuth (no
id_token), so identity comes from its userinfo endpoints. This is the exact
shape the mapping emits, so the app can validate against it precisely.

## Functions

### normalizeGithubProfile()

```ts
function normalizeGithubProfile(userInfoResponses): object;
```

Map GitHub's userinfo responses to [GithubProfile](#githubprofile).

#### Parameters

| Parameter           | Type                                               |
| ------------------- | -------------------------------------------------- |
| `userInfoResponses` | [`GithubUserInfo`](#githubuserinfo) \| `undefined` |

#### Returns

`object`

##### avatarUrl?

```ts
optional avatarUrl?: string;
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

##### login

```ts
login: string;
```

##### name?

```ts
optional name?: string;
```

---

### setupGithub()

```ts
function setupGithub<UsersTable>(core, options): object;
```

Built-in sign-in method for GitHub. Wire it up with the GitHub oauth component:

```ts
const auth = setupConvexAuth({ component: components.auth });
export const { signOut, refreshSession, isAuthenticated } = auth;

export const { startSignInWithGithub, completeSignInWithGithub } = setupGithub(
  auth,
  {
    component: components.authGithub,
    allowedRedirectOrigins: [
      "https://app.example.com",
      "http://localhost:5173",
    ],
  },
).attachUserCallbacks({ createUser: internal.users.createUserGithub });
```

Setup:

1. Install the component in convex.config.ts under
   `httpPrefix: "/oauth/github"`, binding GitHub's `CLIENT_ID` and
   `CLIENT_SECRET`.
2. Register `<site-url>/oauth/github/callback` as the redirect URI with GitHub.

The `httpPrefix` alone determines the callback URL.

#### Type Parameters

| Type Parameter                  |
| ------------------------------- |
| `UsersTable` _extends_ `string` |

#### Parameters

| Parameter | Type                                                            |
| --------- | --------------------------------------------------------------- |
| `core`    | [`AuthCore`](../../../server/index.md#authcore)\<`UsersTable`\> |
| `options` | [`GithubOptions`](#githuboptions)                               |

#### Returns

##### attachUserCallbacks()

```ts
attachUserCallbacks(callbacks): object;
```

Supply the app's user callbacks (see UserCallbacks for how their args must be
declared) and get the functions of this method to export.

###### Parameters

| Parameter   | Type                                                                                                                                                                             |
| ----------- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `callbacks` | `UserCallbacks`\<`"github"`, \{ `avatarUrl?`: `string`; `email?`: `string`; `emailVerified`: `boolean`; `id`: `string`; `login`: `string`; `name?`: `string`; \}, `UsersTable`\> |

###### Returns

`object`

###### completeSignInWithGithub

```ts
completeSignInWithGithub: RegisteredMutation<
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

###### startSignInWithGithub

```ts
startSignInWithGithub: RegisteredMutation<
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
