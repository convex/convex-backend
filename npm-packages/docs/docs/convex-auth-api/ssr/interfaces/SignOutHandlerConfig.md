---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: SignOutHandlerConfig
---

import Link from "@docusaurus/Link";

Configuration for [signOutHandler](../index.md#signouthandler).

## Properties

| Property                                                | Type                                                 | Description                                                                                                                                                                                             |
| ------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedorigins" /> `allowedOrigins?` | `string`[]                                           | Origins beyond the request's own `Host` trusted by the CSRF origin check, for deployments where the two differ (e.g. a proxy that rewrites `Host`). See [isTrustedOrigin](../index.md#istrustedorigin). |
| <Link id="property-convexurl" /> `convexUrl`            | `string`                                             | The Convex deployment URL used server-side.                                                                                                                                                             |
| <Link id="property-cookieoptions" /> `cookieOptions`    | [`AuthCookieOptions`](../index.md#authcookieoptions) | Auth cookie attributes; `secure` is required.                                                                                                                                                           |
| <Link id="property-signout" /> `signOut`                | [`SignOutFn`](../index.md#signoutfn)                 | The app's `signOut` mutation reference.                                                                                                                                                                 |
