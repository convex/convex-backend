---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ConvexProxyConfig
---

import Link from "@docusaurus/Link";

Configuration for [convexProxyHandler](../index.md#convexproxyhandler).

## Properties

| Property                                                | Type                                                 | Description                                                                                                                                  |
| ------------------------------------------------------- | ---------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-allowedorigins" /> `allowedOrigins?` | `string`[]                                           | Origins beyond the request's own `Host` trusted by the CSRF origin check. See [isTrustedOrigin](../index.md#istrustedorigin).                |
| <Link id="property-convexurl" /> `convexUrl`            | `string`                                             | The Convex deployment URL to forward to.                                                                                                     |
| <Link id="property-cookieoptions" /> `cookieOptions`    | [`AuthCookieOptions`](../index.md#authcookieoptions) | Auth cookie attributes; `secure` is required.                                                                                                |
| <Link id="property-signin" /> `signIn`                  | [`ExposedSignInFn`](../index.md#exposedsigninfn)[]   | The sign-in functions reachable through the proxy. Anything not listed here is refused, so this allowlist is the proxy's entire API surface. |
