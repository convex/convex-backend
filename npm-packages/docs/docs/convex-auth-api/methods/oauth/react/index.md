---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: methods/oauth/react
sidebar_label: react
---

import Link from "@docusaurus/Link";

React client for the shared OAuth state, exported at
`@convex-dev/auth/methods/oauth/react`.

OAuth is registered by default in `ConvexAuthProvider` and
`ConvexAuthNextjsProvider`. If you set their `ambientSignIns` prop, include
[oauth](#oauth) yourself. The Google, Apple and GitHub methods have their own
hooks, at `@convex-dev/auth/methods/<provider>/react`. [useOauth](#useoauth)
returns the state that isn't tied to one provider.

Under Next.js, add each provider's `completeSignIn*` function to the auth
proxy's `signIn` allowlist. Its `startSignIn*` function runs on the Convex
client and is not listed.

Errors come back as typed results for the app to switch on, as with the password
hooks. Starting a flow resolves to its result, and a failure after the redirect
back shows up in [useOauth](#useoauth)'s `flowError`.

```tsx
const { flowError } = useOauth();
```

## Type Aliases

### OauthActions

```ts
type OauthActions = object;
```

The sign-in actions [oauth](#oauth) publishes for its hooks to read.

#### Properties

| Property                               | Type                                                                                                                              | Description                                                                                                                                                                                                                                                        |
| -------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| <Link id="property-signin" /> `signIn` | (`refs`, `options?`) => `Promise`\< \| [`OauthStartResult`](#oauthstartresult) \| [`OauthCompleteResult`](#oauthcompleteresult)\> | Start the given provider's OAuth flow, or finish a saved one when `options.code` is set. Starting navigates away to the identity provider. Resolves to an [OauthStartResult](#oauthstartresult) or, with a `code`, an [OauthCompleteResult](#oauthcompleteresult). |

---

### OauthCompleteResult

```ts
type OauthCompleteResult =
  | {
      status: "complete";
    }
  | {
      status: "error";
      userError: OauthFlowError;
    };
```

What finishing a saved flow with a callback `code` resolves to. It never
rejects. The failures are those of [OauthFlowError](#oauthflowerror).

---

### OauthFlowError

```ts
type OauthFlowError =
  | {
      error: "ACCESS_DENIED";
    }
  | {
      error: "EXPIRED";
    }
  | {
      error: "INVALID_FLOW";
    }
  | {
      cause: unknown;
      error: "OTHER_ERROR";
    };
```

Why a flow failed after the identity provider sent the user back. The callback
lands on a page load with no caller waiting on it, so the hooks report it as
`flowError`. Apps switch on `error` and supply their own text for each code.

- `"ACCESS_DENIED"`: the user cancelled at the identity provider.
- `"EXPIRED"`: the flow took too long, or the code was already redeemed.
- `"INVALID_FLOW"`: the callback arrived but this client has no saved flow, so
  it never started this sign-in or already finished it.
- `"OTHER_ERROR"`: anything else, like the identity provider failing the
  handshake or the redemption throwing. `cause` is the thrown value, or the
  error the callback URL carried. When the app's own backend turns the sign-in
  down by throwing a `ConvexError` (from `onSignIn`, say), `cause` is that
  `ConvexError`, as with the other sign-in methods.

---

### OauthProviderApi

```ts
type OauthProviderApi = object;
```

The mutations that the method of an OAuth provider adds to the app's API. Passed
as references, not names, because an app can re-export them under any name.

#### Properties

| Property                                               | Type                                                                                                                                                                    | Description                                |
| ------------------------------------------------------ | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------ |
| <Link id="property-completesignin" /> `completeSignIn` | `FunctionReference`\<`"mutation"`, `"public"`, \{ `code`: `string`; `state`: `string`; \}, [`ClientView`](../../../ssr/index.md#clientview)\<`CompleteSignInResult`\>\> | The provider's `completeSignIn*` mutation. |
| <Link id="property-startsignin" /> `startSignIn`       | `FunctionReference`\<`"mutation"`, `"public"`, \{ `redirectTo`: `string`; \}, \{ `redirect`: `string`; `state`: `string`; \}\>                                          | The provider's `startSignIn*` mutation.    |

---

### OauthProviderRefs

```ts
type OauthProviderRefs = object & OauthProviderApi;
```

One provider's mutations plus its name.

#### Type Declaration

##### providerName

```ts
providerName: string;
```

The provider's name, e.g. `"google"`. Only a label, never a lookup key.

---

### OauthStartResult

```ts
type OauthStartResult =
  | {
      redirect: URL;
      status: "redirect";
    }
  | {
      status: "error";
      userError: {
        cause: unknown;
        error: "OTHER_ERROR";
      };
    };
```

What starting a flow resolves to. A failure comes back as `OTHER_ERROR` with the
thrown value on `cause`, as with the password hooks, so the caller handles every
outcome in one switch with no `try`/`catch`. It only rejects when `redirectTo`
is missing where there is no page URL, which is a bug in the calling code.

On the web the client navigates to `redirect` itself. React Native has no page
to leave, so it opens `redirect` in an in-app browser.

---

### SignInOptions

```ts
type SignInOptions = object;
```

Options accepted by [OauthActions.signIn](#property-signin).

#### Properties

| Property                                        | Type     | Description                                                                                                                                                                                                                       |
| ----------------------------------------------- | -------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-code" /> `code?`             | `string` | A callback `code` to finish a saved flow instead of starting a new one.                                                                                                                                                           |
| <Link id="property-redirectto" /> `redirectTo?` | `string` | Where the flow returns to when it finishes. Defaults to the current URL, and is required where there is no current URL, such as React Native. Must be an http or https URL. Custom schemes like `myapp://` are not supported yet. |

---

### UseOauthReturn

```ts
type UseOauthReturn = object;
```

What [useOauth](#useoauth) returns.

#### Properties

| Property                                     | Type                                          | Description                                                                                                                                        |
| -------------------------------------------- | --------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-flowerror" /> `flowError` | [`OauthFlowError`](#oauthflowerror) \| `null` | Why the last flow failed after the redirect back, or `null`. Cleared on the next sign-in. Switch on its `error` and supply the text for each code. |

---

### UseOauthSignInReturn

```ts
type UseOauthSignInReturn = object;
```

What the sign-in hooks of the OAuth providers return.

#### Properties

| Property                                 | Type                                                                                                                                               | Description                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                     |
| ---------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-signin-1" /> `signIn` | \{ (`options`): `Promise`\<[`OauthCompleteResult`](#oauthcompleteresult)\>; (`options?`): `Promise`\<[`OauthStartResult`](#oauthstartresult)\>; \} | Start the provider's OAuth flow (or, with `options.code`, complete one started elsewhere). Navigates away to the identity provider on the web. React Native isn't supported yet. It gets the `redirect` URL back to open in an in-app browser, but `options.redirectTo` is required there and can only be an http or https URL (see [SignInOptions](#signinoptions)). Starting is declared last so `ReturnType` gives its result. Its `code` is `undefined` so options that might hold a code match neither shape, rather than being typed as a start that could finish a flow. |

## Functions

### oauth()

```ts
function oauth(): AmbientSignInClient;
```

Client setup for the OAuth providers. It owns the flow's storage and the startup
work that finishes a flow. Provider mutations arrive with each
[OauthActions.signIn](#property-signin) call, so the setup takes no
configuration.

#### Returns

`AmbientSignInClient`

---

### useOauth()

```ts
function useOauth(): UseOauthReturn;
```

Read OAuth state that isn't tied to one provider. A flow completes on whichever
page it redirects back to, so `flowError` can appear on a page that never calls
a sign-in hook. An app-level error banner can read it here without any
provider's function references.

#### Returns

[`UseOauthReturn`](#useoauthreturn)
