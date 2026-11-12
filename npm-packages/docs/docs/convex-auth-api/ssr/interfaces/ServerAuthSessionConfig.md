---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ServerAuthSessionConfig
---

import Link from "@docusaurus/Link";

Configuration for a [ServerAuthSession](../classes/ServerAuthSession.md).

## Properties

| Property                                                        | Type                                                 | Description                                                                                                                                                  |
| --------------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| <Link id="property-cookieoptions" /> `cookieOptions`            | [`AuthCookieOptions`](../index.md#authcookieoptions) | Base cookie attributes. Per-cookie lifetime is derived from the token bundle. `secure` is required (see [AuthCookieOptions](../index.md#authcookieoptions)). |
| <Link id="property-cookies" /> `cookies`                        | [`CookieStore`](CookieStore.md)                      | Request/response cookies for this SSR request.                                                                                                               |
| <Link id="property-refreshsession" /> `refreshSession`          | [`RefreshSession`](../index.md#refreshsession)       | Exchange the cookie's refresh token for a fresh session (typically over a Convex HTTP client).                                                               |
| <Link id="property-refreshskewseconds" /> `refreshSkewSeconds?` | `number`                                             | Seconds before access-token expiry at which [ServerAuthSession.getToken](../classes/ServerAuthSession.md#gettoken) proactively refreshes. Defaults to 10.    |
