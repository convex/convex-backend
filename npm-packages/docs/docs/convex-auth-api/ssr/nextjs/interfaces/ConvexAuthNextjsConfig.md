---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ConvexAuthNextjsConfig
---

import Link from "@docusaurus/Link";

Configuration for [setupConvexAuthNextjs](../index.md#setupconvexauthnextjs).

## Properties

| Property                                                 | Type                                                                 | Description                                                                                                                                                                                                                                                                                                                                                                                 |
| -------------------------------------------------------- | -------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-convexurl" /> `convexUrl`             | `string`                                                             | The Convex deployment URL used server-side.                                                                                                                                                                                                                                                                                                                                                 |
| <Link id="property-cookieoptions" /> `cookieOptions?`    | `Partial`\<[`AuthCookieOptions`](../../index.md#authcookieoptions)\> | Overrides the default auth cookie attributes. Unlike the base config, `secure` may be omitted here; it defaults from NODE_ENV.                                                                                                                                                                                                                                                              |
| <Link id="property-isauthenticated" /> `isAuthenticated` | [`IsAuthenticatedFn`](../../index.md#isauthenticatedfn)              | The app's `isAuthenticated` query reference, used twice over: the server verifies the access token against the backend with it (which checks its signature and that the session is live) rather than trusting the cookie by decoding it, and the client provider subscribes to it, so a session revoked server-side signs the browser out reactively instead of at the next failed refresh. |
| <Link id="property-refreshsession" /> `refreshSession`   | [`RefreshSessionFn`](../../index.md#refreshsessionfn)                | The app's `refreshSession` mutation reference. The proxy uses it for its optimistic refresh.                                                                                                                                                                                                                                                                                                |
