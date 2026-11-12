---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ServerAuthSession
---

## Constructors

### Constructor

```ts
new ServerAuthSession(config): ServerAuthSession;
```

#### Parameters

| Parameter | Type                                                                  |
| --------- | --------------------------------------------------------------------- |
| `config`  | [`ServerAuthSessionConfig`](../interfaces/ServerAuthSessionConfig.md) |

#### Returns

`ServerAuthSession`

## Methods

### getToken()

```ts
getToken(): Promise<string | null>;
```

The access token to authenticate SSR requests with. Returns the cookie's token
when it is still comfortably valid; otherwise refreshes (rotating the refresh
token and rewriting both cookies). `null` means no usable session.

#### Returns

`Promise`\<`string` \| `null`\>

---

### refresh()

```ts
refresh(): Promise<
  | {
  kind: "rotated";
  tokens: {
     accessToken: string;
     refreshToken: string;
     refreshTokenExpiresAt: number;
     userId: string;
  };
}
  | {
  kind: "noSession";
}>;
```

Force a refresh from the refresh-token cookie, updating the cookies to match the
outcome:

- `rotated`: rewrite both cookies with the new bundle.
- `noSession`: clear both.

#### Returns

`Promise`\< \| \{ `kind`: `"rotated"`; `tokens`: \{ `accessToken`: `string`;
`refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`;
\}; \} \| \{ `kind`: `"noSession"`; \}\>

---

### setTokens()

```ts
setTokens(bundle): Promise<void>;
```

Persist a token bundle as cookies. Used after a sign-in whose bundle was minted
elsewhere (e.g. a client-side sign-in flow) to move the refresh token into the
httpOnly cookie. Both cookies live as long as the refresh token; the access
token is refreshed on expiry via [getToken](#gettoken).

#### Parameters

| Parameter                      | Type                                                                                                            |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `bundle`                       | \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \} |
| `bundle.accessToken`           | `string`                                                                                                        |
| `bundle.refreshToken`          | `string`                                                                                                        |
| `bundle.refreshTokenExpiresAt` | `number`                                                                                                        |
| `bundle.userId`                | `string`                                                                                                        |

#### Returns

`Promise`\<`void`\>
