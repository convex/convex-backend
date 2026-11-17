---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ConvexAuthServerConfig
---

import Link from "@docusaurus/Link";

Configuration for [setupConvexAuthServer](../index.md#setupconvexauthserver).

## Properties

| Property                                                | Type                                                 | Description                                                                                                                                                                                                                                                                                                                                                                       |
| ------------------------------------------------------- | ---------------------------------------------------- | --------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedorigins" /> `allowedOrigins?` | `string`[]                                           | Origins beyond the request's own `Host` trusted by every handler's CSRF origin check, for deployments where the two differ (e.g. a proxy that rewrites `Host`). See [isTrustedOrigin](../index.md#istrustedorigin).                                                                                                                                                               |
| <Link id="property-convexurl" /> `convexUrl`            | `string`                                             | The Convex deployment URL used server-side.                                                                                                                                                                                                                                                                                                                                       |
| <Link id="property-cookieoptions" /> `cookieOptions`    | [`AuthCookieOptions`](../index.md#authcookieoptions) | Auth cookie attributes for every handler; `secure` is required.                                                                                                                                                                                                                                                                                                                   |
| <Link id="property-refreshsession" /> `refreshSession`  | [`RefreshSessionFn`](../index.md#refreshsessionfn)   | The app's `refreshSession` mutation reference.                                                                                                                                                                                                                                                                                                                                    |
| <Link id="property-signin" /> `signIn`                  | [`ExposedSignInFn`](../index.md#exposedsigninfn)[]   | The sign-in functions of the methods to expose through the auth proxy. This should include all the Convex backend functions in your app that can issue auth sessions (e.g. account creation, log in with existing account…). Anything not listed is refused, so this allowlist is the proxy's entire API surface. Listing a function here is all the wiring an auth method needs. |
| <Link id="property-signout" /> `signOut`                | [`SignOutFn`](../index.md#signoutfn)                 | The app's `signOut` mutation reference.                                                                                                                                                                                                                                                                                                                                           |
