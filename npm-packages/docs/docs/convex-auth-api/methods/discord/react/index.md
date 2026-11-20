---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/discord/react
sidebar_label: react
---

import Link from "@docusaurus/Link";

React client for the Discord sign-in method, exported at
`@convex-dev/auth/methods/discord/react`.

[useSignInWithDiscord](#usesigninwithdiscord) starts the sign-in.
[useOauth](../../oauth/react/index.md#useoauth) returns the state that is shared
by all the OAuth methods, for example the error of the last flow.

```tsx
const { signIn } = useSignInWithDiscord(api.auth);
const { flowError } = useOauth();
```

## Type Aliases

### UseSignInWithDiscordReturn

```ts
type UseSignInWithDiscordReturn = object;
```

What [useSignInWithDiscord](#usesigninwithdiscord) returns.

#### Properties

| Property                               | Type                                                                                    | Description                                                                                                  |
| -------------------------------------- | --------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------ |
| <Link id="property-signin" /> `signIn` | [`UseOauthSignInReturn`](../../oauth/react/index.md#useoauthsigninreturn)\[`"signIn"`\] | Start Discord's OAuth flow. See [UseOauthSignInReturn.signIn](../../oauth/react/index.md#property-signin-1). |

## Functions

### useSignInWithDiscord()

```ts
function useSignInWithDiscord(api): UseSignInWithDiscordReturn;
```

Sign in with Discord. Pass the module exporting the functions of the method
(usually the generated `api.auth`), or an object mapping the canonical keys to
renamed exports.

#### Parameters

| Parameter                       | Type                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ------------------------------- | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `api`                           | \{ `completeSignInWithDiscord`: `FunctionReference`\<`"mutation"`, `"public"`, \{ `code`: `string`; `state`: `string`; \}, [`ClientView`](../../../ssr/index.md#clientview)\< \| \{ `status`: `"complete"`; `tokens`: \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{ `status`: `"error"`; `userError`: \{ `error`: `"INVALID_CODE"`; \}; \}\>\>; `startSignInWithDiscord`: `FunctionReference`\<`"mutation"`, `"public"`, \{ `redirectTo`: `string`; \}, \{ `redirect`: `string`; `state`: `string`; \}\>; \} |
| `api.completeSignInWithDiscord` | `FunctionReference`\<`"mutation"`, `"public"`, \{ `code`: `string`; `state`: `string`; \}, [`ClientView`](../../../ssr/index.md#clientview)\< \| \{ `status`: `"complete"`; `tokens`: \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{ `status`: `"error"`; `userError`: \{ `error`: `"INVALID_CODE"`; \}; \}\>\>                                                                                                                                                                                               |
| `api.startSignInWithDiscord`    | `FunctionReference`\<`"mutation"`, `"public"`, \{ `redirectTo`: `string`; \}, \{ `redirect`: `string`; `state`: `string`; \}\>                                                                                                                                                                                                                                                                                                                                                                                                                                                           |

#### Returns

[`UseSignInWithDiscordReturn`](#usesigninwithdiscordreturn)

## References

### OauthCompleteResult

Re-exports [OauthCompleteResult](../../oauth/react/index.md#oauthcompleteresult)

---

### OauthFlowError

Re-exports [OauthFlowError](../../oauth/react/index.md#oauthflowerror)

---

### OauthStartResult

Re-exports [OauthStartResult](../../oauth/react/index.md#oauthstartresult)

---

### SignInOptions

Re-exports [SignInOptions](../../oauth/react/index.md#signinoptions)

---

### useOauth

Re-exports [useOauth](../../oauth/react/index.md#useoauth)

---

### UseOauthReturn

Re-exports [UseOauthReturn](../../oauth/react/index.md#useoauthreturn)
