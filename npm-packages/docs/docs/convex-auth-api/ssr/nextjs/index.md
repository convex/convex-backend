---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ssr/nextjs
sidebar_label: nextjs
---

Server bindings for Convex Auth on Next.js (App Router), exported at
`@convex-dev/auth/ssr/nextjs`.

This is the thin, genuinely Next-specific layer. The auth _handlers_ (per-method
sign-in, plus refresh/sign-out) are framework-agnostic `(Request) => Response`
functions mounted directly as route handlers (see `@convex-dev/auth/ssr`). What
remains Next-specific is the proxy (up-front refresh + redirects), reading the
access token in Server Components, and the server-side provider that hydrates
the client.

## Interfaces

- [ConvexAuthNextjsConfig](interfaces/ConvexAuthNextjsConfig.md)
- [ConvexAuthNextjsProxyCtx](interfaces/ConvexAuthNextjsProxyCtx.md)

## Type Aliases

### ConvexAuthNextjsProxyHandler

```ts
type ConvexAuthNextjsProxyHandler = (
  request,
  ctx,
) => Promise<NextResponse | undefined | void> | NextResponse | undefined | void;
```

A proxy handler: inspect the request, optionally return a response (e.g. a
redirect) to short-circuit.

#### Parameters

| Parameter | Type                                                                 |
| --------- | -------------------------------------------------------------------- |
| `request` | `NextRequest`                                                        |
| `ctx`     | [`ConvexAuthNextjsProxyCtx`](interfaces/ConvexAuthNextjsProxyCtx.md) |

#### Returns

\| `Promise`\<`NextResponse` \| `undefined` \| `void`\> \| `NextResponse` \|
`undefined` \| `void`

## Functions

### setupConvexAuthNextjs()

```ts
function setupConvexAuthNextjs(config): object;
```

Build the Next.js server helpers from one config.

```ts
export const {
  convexAuthNextjsProxy,
  nextjsProxyRedirect,
  convexAuthNextjsAccessToken,
  isAuthenticatedNextjs,
  ConvexAuthNextjsServerProvider,
} = setupConvexAuthNextjs({
  convexUrl: process.env.NEXT_PUBLIC_CONVEX_URL!,
  refreshSession: api.auth.refreshSession,
  isAuthenticated: api.auth.isAuthenticated,
});
```

#### Parameters

| Parameter | Type                                                             |
| --------- | ---------------------------------------------------------------- |
| `config`  | [`ConvexAuthNextjsConfig`](interfaces/ConvexAuthNextjsConfig.md) |

#### Returns

##### convexAuthNextjsAccessToken

```ts
convexAuthNextjsAccessToken: () => Promise<string | null>;
```

The current access token in a Server Component, or null. Never triggers a
refresh (the proxy does that); returns null for an expired token.

The token is returned after only a local expiry check, not a signature check —
that is safe because its only use is to authenticate a request to the Convex
backend (e.g. `preloadQuery`), which verifies the signature itself. To decide
_authentication state_ server-side, use isAuthenticatedNextjs instead, which
verifies against the backend.

###### Returns

`Promise`\<`string` \| `null`\>

##### convexAuthNextjsProxy

```ts
convexAuthNextjsProxy: (handler?) => (request) =>
  Promise<NextResponse<unknown>>;
```

Build a Next.js proxy (`proxy.ts`) that refreshes the session up front (so
downstream Server Components see a fresh token) and optionally runs redirect
logic.

###### Parameters

| Parameter  | Type                                                            |
| ---------- | --------------------------------------------------------------- |
| `handler?` | [`ConvexAuthNextjsProxyHandler`](#convexauthnextjsproxyhandler) |

###### Returns

(`request`) => `Promise`\<`NextResponse`\<`unknown`\>\>

##### ConvexAuthNextjsServerProvider

```ts
ConvexAuthNextjsServerProvider: (__namedParameters) => Promise<Element>;
```

Server Component that reads the cookie token and renders the client provider, so
the client hydrates ready to authenticate.

###### Parameters

| Parameter                    | Type                           |
| ---------------------------- | ------------------------------ |
| `__namedParameters`          | \{ `children`: `ReactNode`; \} |
| `__namedParameters.children` | `ReactNode`                    |

###### Returns

`Promise`\<`Element`\>

##### isAuthenticatedNextjs

```ts
isAuthenticatedNextjs: () => Promise<boolean>;
```

Whether the current request carries a valid, signed-in session. Verifies the
cookie's access token against the backend (which checks its signature), so a
forged cookie can't fake authentication.

###### Returns

`Promise`\<`boolean`\>

##### nextjsProxyRedirect

```ts
nextjsProxyRedirect: (request, path) => NextResponse;
```

Redirect helper for use inside a proxy handler.

Part of the API returned from [setupConvexAuthNextjs](#setupconvexauthnextjs).

###### Parameters

| Parameter | Type          |
| --------- | ------------- |
| `request` | `NextRequest` |
| `path`    | `string`      |

###### Returns

`NextResponse`
