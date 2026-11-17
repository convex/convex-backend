---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ssr
---

import Link from "@docusaurus/Link";

Framework-agnostic server (SSR) building blocks for Convex Auth: a cookie
abstraction, a WHATWG `Request`/`Response` cookie adapter, access-token
inspection, the [ServerAuthSession](classes/ServerAuthSession.md) that owns the
refresh lifecycle, the [createServerAuthChecker](#createserverauthchecker) that
verifies an access token against the backend, the method-agnostic
`(Request) => Response` refresh/sign-out handlers, the
[convexProxyHandler](#convexproxyhandler) that serves sign-in, and the
[setupConvexAuthServer](#setupconvexauthserver) factory that configures them all
once.

## Classes

- [ServerAuthSession](classes/ServerAuthSession.md)

## Interfaces

- [ConvexAuthServerConfig](interfaces/ConvexAuthServerConfig.md)
- [ConvexProxyConfig](interfaces/ConvexProxyConfig.md)
- [CookieOptions](interfaces/CookieOptions.md)
- [CookieStore](interfaces/CookieStore.md)
- [DecodedAccessToken](interfaces/DecodedAccessToken.md)
- [HttpCookies](interfaces/HttpCookies.md)
- [RefreshHandlerConfig](interfaces/RefreshHandlerConfig.md)
- [ServerAuthCheckerConfig](interfaces/ServerAuthCheckerConfig.md)
- [ServerAuthSessionConfig](interfaces/ServerAuthSessionConfig.md)
- [SignOutHandlerConfig](interfaces/SignOutHandlerConfig.md)

## Type Aliases

### AuthCookieOptions

```ts
type AuthCookieOptions = CookieDeleteOptions & object;
```

The cookie options a framework integration may supply for auth cookies: only the
attributes that legitimately vary by deployment.

`secure` is required. Whether auth cookies are HTTPS-only depends on the
deployment (HTTPS in production, plain http on localhost), so every integration
has to decide it explicitly rather than inherit a default that would be wrong in
one environment or the other.

The security-relevant attributes (`httpOnly`, `sameSite`) and the token
lifetimes (`maxAge`, `expires`, which derive from the token bundle) are
deliberately not configurable; [writeAuthCookies](#writeauthcookies) always
applies the invariant values.

#### Type Declaration

##### secure

```ts
secure: boolean;
```

---

### AuthSessionResponse

```ts
type AuthSessionResponse = object;
```

The JSON body of every SSR auth session response: the refresh handler, the
sign-out handler, and the cross-site refusal all reply with this shape, and the
client parses it without looking at the status. `tokens` is null whenever there
is no live session to report (a dead or missing refresh cookie, a completed
sign-out, a refused origin).

#### Properties

| Property                               | Type                                            |
| -------------------------------------- | ----------------------------------------------- |
| <Link id="property-tokens" /> `tokens` | [`SlimTokenBundle`](#slimtokenbundle) \| `null` |

---

### ClientView

```ts
type ClientView<T> = T extends object ? Omit<T, "tokens"> & object : T;
```

A sign-in result as clients see it, with the token bundle narrowed to what every
session model delivers.

Under SSR the auth proxy strips the refresh token before the response reaches
the browser, so the slim bundle is all the two models have in common. Declaring
that here makes the stripping a widening of the value rather than a broken
contract: a [TokenBundle](../react/index.md#tokenbundle) is assignable to a
[SlimTokenBundle](#slimtokenbundle), so one type stays true for both.

It also stops callers depending on the refresh token. Handing the bundle to
`setSession` is the only supported use; an app that needs the raw token can call
its Convex function with the generated reference and get the full type.

Distributes over a result union, leaving failure arms untouched.

#### Type Parameters

| Type Parameter |
| -------------- |
| `T`            |

---

### CookieDeleteOptions

```ts
type CookieDeleteOptions = Pick<CookieOptions, "path" | "domain">;
```

The attributes that identify which cookie a deletion targets: browsers only
remove a cookie when the deletion's `path` and `domain` match the ones it was
written with. No other attribute takes part in matching.

---

### ExposedSignInFn

```ts
type ExposedSignInFn = FunctionReference<
  "mutation" | "action",
  "public",
  DefaultFunctionArgs,
  SignInEnvelope
>;
```

A sign-in function exposed through the proxy.

Any public mutation or action returning the shared
[SignInEnvelope](#signinenvelope). Args are typed loosely because the proxy
never inspects them; it forwards the caller's encoded args untouched.

---

### IsAuthenticatedFn

```ts
type IsAuthenticatedFn = FunctionReference<
  "query",
  "public",
  Record<string, never>,
  boolean
>;
```

The app's `isAuthenticated` query reference: reports whether the access token
the query is called with identifies a signed-in user.

---

### RefreshResult

```ts
type RefreshResult = Infer<typeof vRefreshResult>;
```

The result of refreshing a session, returned by the app's `refreshSession`
mutation and by `ServerAuthSession.refresh()`. Check `kind`:

- `{ kind: "rotated", tokens }`: `tokens` is the new
  [TokenBundle](../react/index.md#tokenbundle), with a fresh access token and
  the refresh token that replaces the one presented. Persist both. The presented
  refresh token stays valid until its successor is first used, so a client that
  never receives this result can retry with the token it already has.
- `{ kind: "noSession" }`: the refresh token is unknown, expired, or superseded
  (a token issued after it has already been used). Only a superseded token
  revokes anything: it is what a leaked token looks like, so presenting it ends
  its session for every holder. An unknown or expired token has no live session
  left to revoke. Clear the stored session and treat the user as signed out.

---

### RefreshSession

```ts
type RefreshSession = (refreshToken) => Promise<RefreshResult>;
```

Exchange a refresh token for a fresh session, resolving to one of the
[RefreshResult](#refreshresult) outcomes. A framework binding usually implements
this over a Convex HTTP client; tests pass a fake.

#### Parameters

| Parameter      | Type     |
| -------------- | -------- |
| `refreshToken` | `string` |

#### Returns

`Promise`\<[`RefreshResult`](#refreshresult)\>

---

### RefreshSessionFn

```ts
type RefreshSessionFn = FunctionReference<
  "mutation",
  "public",
  {
    refreshToken: string;
  },
  RefreshResult
>;
```

The app's `refreshSession` mutation reference: exchange a refresh token for a
fresh session. See [RefreshResult](#refreshresult) for the outcomes.

---

### RequestHandler

```ts
type RequestHandler = (request) => Promise<Response>;
```

A WHATWG request handler.

#### Parameters

| Parameter | Type      |
| --------- | --------- |
| `request` | `Request` |

#### Returns

`Promise`\<`Response`\>

---

### ServerAuthChecker

```ts
type ServerAuthChecker = (accessToken) => Promise<boolean>;
```

Reports whether a JWT `accessToken` is a valid, signed-in session (`null` = no
token = not signed in).

#### Parameters

| Parameter     | Type               |
| ------------- | ------------------ |
| `accessToken` | `string` \| `null` |

#### Returns

`Promise`\<`boolean`\>

---

### SignInComplete

```ts
type SignInComplete = Infer<typeof vSignInComplete>;
```

---

### SignInEnvelope

```ts
type SignInEnvelope = SignInComplete | SignInError<unknown>;
```

The full shared sign-in envelope.

Represents sign-ins that ran to completion and those that hit an error.

`userError` is `unknown` here because each method declares its own codes.

---

### SignInError

```ts
type SignInError<UserError> = object;
```

The inferred shape of a vSignInError arm.

#### Type Parameters

| Type Parameter |
| -------------- |
| `UserError`    |

#### Properties

| Property                                     | Type        |
| -------------------------------------------- | ----------- |
| <Link id="property-status" /> `status`       | `"error"`   |
| <Link id="property-usererror" /> `userError` | `UserError` |

---

### SignInStatus

```ts
type SignInStatus = SignInEnvelope["status"];
```

The `status` discriminant of every [SignInEnvelope](#signinenvelope).

---

### SignOutFn

```ts
type SignOutFn = FunctionReference<
  "mutation",
  "public",
  {
    refreshToken: string;
  },
  null
>;
```

The app's `signOut` mutation reference: revoke the session for a refresh token.

---

### SlimTokenBundle

```ts
type SlimTokenBundle = object;
```

The token bundle that an SSR route hands back to the client. It is a slimmed
down version of a [TokenBundle](../react/index.md#tokenbundle) with the refresh
token (and its expiry) removed: under SSR the refresh token lives only in an
httpOnly cookie and never reaches client JS.

#### Properties

| Property                                         | Type     |
| ------------------------------------------------ | -------- |
| <Link id="property-accesstoken" /> `accessToken` | `string` |
| <Link id="property-userid" /> `userId`           | `string` |

## Variables

### AUTH_JWT_COOKIE

```ts
const AUTH_JWT_COOKIE: "__convexAuthJWT" = JWT_STORAGE_KEY;
```

The cookie holding the current access token (a JWT).

---

### AUTH_REFRESH_COOKIE

```ts
const AUTH_REFRESH_COOKIE: "__convexAuthRefreshToken" =
  REFRESH_TOKEN_STORAGE_KEY;
```

The cookie holding the current (rotating) refresh token.

## Functions

### clearAuthCookies()

```ts
function clearAuthCookies(cookies, options): Promise<void>;
```

Delete both auth cookies. The counterpart of
[writeAuthCookies](#writeauthcookies): it takes the same options so the
deletions match the cookies the writes produced (a deletion only removes a
cookie whose `path`/`domain` match).

#### Parameters

| Parameter | Type                                       |
| --------- | ------------------------------------------ |
| `cookies` | [`CookieStore`](interfaces/CookieStore.md) |
| `options` | [`AuthCookieOptions`](#authcookieoptions)  |

#### Returns

`Promise`\<`void`\>

---

### convexProxyHandler()

```ts
function convexProxyHandler(config): RequestHandler;
```

Build the route that serves every allowlisted sign-in function.

Mount it once, at a static path of your choosing. Adding an auth method means
adding its function to
[ConvexProxyConfig.signIn](interfaces/ConvexProxyConfig.md#property-signin), not
adding another route.

```ts
// app/auth/signin/route.ts
import { auth } from "@/auth"; // the result of `setupConvexAuthServer`

export const POST = auth.convexProxyHandler;
```

#### Parameters

| Parameter | Type                                                   |
| --------- | ------------------------------------------------------ |
| `config`  | [`ConvexProxyConfig`](interfaces/ConvexProxyConfig.md) |

#### Returns

[`RequestHandler`](#requesthandler)

---

### createServerAuthChecker()

```ts
function createServerAuthChecker(config): ServerAuthChecker;
```

Build a [ServerAuthChecker](#serverauthchecker) that verifies a JWT access token
against the backend.

The returned checker treats an absent or already-expired token as
unauthenticated without a round trip (a cheap local gate); otherwise it asks the
backend, which verifies the token's signature. Any error (network, rejected
token) fails closed to `false`.

#### Parameters

| Parameter | Type                                                               |
| --------- | ------------------------------------------------------------------ |
| `config`  | [`ServerAuthCheckerConfig`](interfaces/ServerAuthCheckerConfig.md) |

#### Returns

[`ServerAuthChecker`](#serverauthchecker)

---

### decodeAccessToken()

```ts
function decodeAccessToken(token): DecodedAccessToken;
```

Decode an access token's claims without verifying its signature. Returns nulls
if the token is malformed.

#### Parameters

| Parameter | Type     |
| --------- | -------- |
| `token`   | `string` |

#### Returns

[`DecodedAccessToken`](interfaces/DecodedAccessToken.md)

---

### httpCookies()

```ts
function httpCookies(request): HttpCookies;
```

Adapt an incoming Request's cookies to a
[CookieStore](interfaces/CookieStore.md).

The returned [HttpCookies](interfaces/HttpCookies.md) object allows applying
changes to the cookies via the
[HttpCookies#applyTo](interfaces/HttpCookies.md#property-applyto) function. Pass
it Response `Headers` and it will write the cookie values.

```ts
const client = new ConvexHttpClient(process.env.CONVEX_URL!);

export async function POST(request: Request) {
  const cookies = httpCookies(request);
  const session = new ServerAuthSession({
    refreshSession: (refreshToken) =>
      client.mutation(api.auth.refreshSession, { refreshToken }),
    cookies,
    cookieOptions: { secure: true },
  });
  const result = await session.refresh();
  const res = Response.json({
    tokens: result.kind === "rotated" ? makeSlimBundle(result.tokens) : null,
  });
  cookies.applyTo(res.headers);
  return res;
}
```

#### Parameters

| Parameter | Type      |
| --------- | --------- |
| `request` | `Request` |

#### Returns

[`HttpCookies`](interfaces/HttpCookies.md)

---

### isTokenExpiring()

```ts
function isTokenExpiring(token, skewSeconds?, nowMs?): boolean;
```

Whether a token is expired or within `skewSeconds` of expiry (so the caller
should refresh). A token whose expiry can't be read is treated as expiring.

#### Parameters

| Parameter     | Type     | Default value |
| ------------- | -------- | ------------- |
| `token`       | `string` | `undefined`   |
| `skewSeconds` | `number` | `10`          |
| `nowMs`       | `number` | `...`         |

#### Returns

`boolean`

---

### isTrustedOrigin()

```ts
function isTrustedOrigin(request, allowedOrigins?): boolean;
```

Whether a request may be served by a state-changing auth handler.

Trusts a request when its `Origin` header matches the `Host` header it arrived
at or one of `allowedOrigins` (for deployments where the two legitimately
differ, e.g. a proxy that rewrites `Host`). Entries may be full origins
(`"https://app.example.com"`) or bare hosts (`"app.example.com"`,
`"localhost:3000"`).

#### Parameters

| Parameter        | Type       | Default value |
| ---------------- | ---------- | ------------- |
| `request`        | `Request`  | `undefined`   |
| `allowedOrigins` | `string`[] | `[]`          |

#### Returns

`boolean`

---

### makeSlimBundle()

```ts
function makeSlimBundle(bundle): SlimTokenBundle;
```

Strip down a [TokenBundle](../react/index.md#tokenbundle) down to a
[SlimTokenBundle](#slimtokenbundle), dropping the refresh token so it is never
sent to the browser.

#### Parameters

| Parameter                      | Type                                                                                                            |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `bundle`                       | \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \} |
| `bundle.accessToken`           | `string`                                                                                                        |
| `bundle.refreshToken`          | `string`                                                                                                        |
| `bundle.refreshTokenExpiresAt` | `number`                                                                                                        |
| `bundle.userId`                | `string`                                                                                                        |

#### Returns

[`SlimTokenBundle`](#slimtokenbundle)

---

### refreshHandler()

```ts
function refreshHandler(config): RequestHandler;
```

A handler that refreshes the session from the httpOnly refresh-token cookie,
replying with an [AuthSessionResponse](#authsessionresponse) carrying the fresh
access token. When there is no session left (missing or unrecognized refresh
cookie) it replies 401 with `tokens: null`.

#### Parameters

| Parameter | Type                                                         |
| --------- | ------------------------------------------------------------ |
| `config`  | [`RefreshHandlerConfig`](interfaces/RefreshHandlerConfig.md) |

#### Returns

[`RequestHandler`](#requesthandler)

---

### serializeCookie()

```ts
function serializeCookie(name, value, options?): string;
```

Serialize a cookie into a `Set-Cookie` header value.

Thin adapter over the `cookie` package's `serialize` that maps our
[CookieOptions](interfaces/CookieOptions.md) onto it.

Deletion is expressed by the caller as an empty value with `maxAge: 0` (and a
past `expires`); see [httpCookies](#httpcookies)'s `delete`.

#### Parameters

| Parameter | Type                                           |
| --------- | ---------------------------------------------- |
| `name`    | `string`                                       |
| `value`   | `string`                                       |
| `options` | [`CookieOptions`](interfaces/CookieOptions.md) |

#### Returns

`string`

---

### setupConvexAuthServer()

```ts
function setupConvexAuthServer(config): object;
```

Configure the auth handlers once and get back the routes to mount.

```ts
// auth.ts
export const auth = setupConvexAuthServer({
  convexUrl: process.env.NEXT_PUBLIC_CONVEX_URL!,
  refreshSession: api.auth.refreshSession,
  signOut: api.auth.signOut,
  signIn: [
    api.auth.signInWithUsernamePassword,
    api.auth.signUpWithUsernamePassword,
  ],
  cookieOptions: { secure: process.env.NODE_ENV === "production" },
});
```

Then mount the routes that it returns. For example, the sign-in route serves
every method listed in `signIn`:

```ts
// app/auth/signin/route.ts
import { auth } from "@/auth"; // the result of `setupConvexAuthServer`

export const POST = auth.convexProxyHandler;
```

#### Parameters

| Parameter | Type                                                             |
| --------- | ---------------------------------------------------------------- |
| `config`  | [`ConvexAuthServerConfig`](interfaces/ConvexAuthServerConfig.md) |

#### Returns

##### convexProxyHandler

```ts
convexProxyHandler: RequestHandler;
```

The sign-in route.

Mount it once, at a static path, and every function in `signIn` is reachable
from the normal client hook of its method.

```ts
// app/auth/signin/route.ts
import { auth } from "@/auth"; // the result of `setupConvexAuthServer`

export const POST = auth.convexProxyHandler;
```

##### refreshHandler

```ts
refreshHandler: RequestHandler;
```

##### signOutHandler

```ts
signOutHandler: RequestHandler;
```

---

### signOutHandler()

```ts
function signOutHandler(config): RequestHandler;
```

A handler that revokes the session from the httpOnly refresh-token cookie (best
effort) and clears both cookies, replying with an
[AuthSessionResponse](#authsessionresponse) carrying `tokens: null`.

#### Parameters

| Parameter | Type                                                         |
| --------- | ------------------------------------------------------------ |
| `config`  | [`SignOutHandlerConfig`](interfaces/SignOutHandlerConfig.md) |

#### Returns

[`RequestHandler`](#requesthandler)

---

### writeAuthCookies()

```ts
function writeAuthCookies(cookies, bundle, options): Promise<void>;
```

Write the tokens in a `bundle` as cookies.

The access token is stored in [AUTH_JWT_COOKIE](#auth_jwt_cookie) and the
refresh token in [AUTH_REFRESH_COOKIE](#auth_refresh_cookie). The invariant
attributes (httpOnly etc.) are merged in, so the refresh cookie is httpOnly and
never reaches client JS.

This is the one place that knows which cookies hold a session, so refresh, the
proxy, and every method's sign-in handler write cookies identically.

#### Parameters

| Parameter                      | Type                                                                                                            |
| ------------------------------ | --------------------------------------------------------------------------------------------------------------- |
| `cookies`                      | [`CookieStore`](interfaces/CookieStore.md)                                                                      |
| `bundle`                       | \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \} |
| `bundle.accessToken`           | `string`                                                                                                        |
| `bundle.refreshToken`          | `string`                                                                                                        |
| `bundle.refreshTokenExpiresAt` | `number`                                                                                                        |
| `bundle.userId`                | `string`                                                                                                        |
| `options`                      | [`AuthCookieOptions`](#authcookieoptions)                                                                       |

#### Returns

`Promise`\<`void`\>

## References

### ConvexAuthApi

Re-exports [ConvexAuthApi](../react/index.md#convexauthapi)

---

### TokenBundle

Re-exports [TokenBundle](../react/index.md#tokenbundle)
