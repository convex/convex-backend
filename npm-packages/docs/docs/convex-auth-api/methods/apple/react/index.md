---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/apple/react
sidebar_label: react
---

import Link from "@docusaurus/Link";

React client for the Apple sign-in method, exported at
`@convex-dev/auth/methods/apple/react`.

[useSignInWithApple](#usesigninwithapple) starts the sign-in.
[useOauth](../../oauth/react/index.md#useoauth) returns the state that is shared
by all the OAuth methods, for example the error of the last flow.

```tsx
const { signIn } = useSignInWithApple(api.auth);
const { flowError } = useOauth();
```

## Type Aliases

### UseSignInWithAppleReturn

```ts
type UseSignInWithAppleReturn = object;
```

What [useSignInWithApple](#usesigninwithapple) returns.

#### Properties

| Property                               | Type                                                                                    | Description                                                                                                |
| -------------------------------------- | --------------------------------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------------- |
| <Link id="property-signin" /> `signIn` | [`UseOauthSignInReturn`](../../oauth/react/index.md#useoauthsigninreturn)\[`"signIn"`\] | Start Apple's OAuth flow. See [UseOauthSignInReturn.signIn](../../oauth/react/index.md#property-signin-1). |

## Functions

### useSignInWithApple()

```ts
function useSignInWithApple(api): UseSignInWithAppleReturn;
```

Sign in with Apple. Pass the module exporting the functions of the method
(usually the generated `api.auth`), or an object mapping the canonical keys to
renamed exports.

#### Parameters

| Parameter                     | Type                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                 |
| ----------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `api`                         | \{ `completeSignInWithApple`: `FunctionReference`\<`"mutation"`, `"public"`, \{ `code`: `string`; `state`: `string`; \}, [`ClientView`](../../../ssr/index.md#clientview)\< \| \{ `status`: `"complete"`; `tokens`: \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{ `status`: `"error"`; `userError`: \{ `error`: `"INVALID_CODE"`; \}; \}\>\>; `startSignInWithApple`: `FunctionReference`\<`"mutation"`, `"public"`, \{ `redirectTo`: `string`; \}, \{ `redirect`: `string`; `state`: `string`; \}\>; \} |
| `api.completeSignInWithApple` | `FunctionReference`\<`"mutation"`, `"public"`, \{ `code`: `string`; `state`: `string`; \}, [`ClientView`](../../../ssr/index.md#clientview)\< \| \{ `status`: `"complete"`; `tokens`: \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{ `status`: `"error"`; `userError`: \{ `error`: `"INVALID_CODE"`; \}; \}\>\>                                                                                                                                                                                           |
| `api.startSignInWithApple`    | `FunctionReference`\<`"mutation"`, `"public"`, \{ `redirectTo`: `string`; \}, \{ `redirect`: `string`; `state`: `string`; \}\>                                                                                                                                                                                                                                                                                                                                                                                                                                                       |

#### Returns

[`UseSignInWithAppleReturn`](#usesigninwithapplereturn)

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
