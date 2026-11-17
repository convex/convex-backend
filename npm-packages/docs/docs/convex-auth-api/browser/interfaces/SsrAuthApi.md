---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: SsrAuthApi
---

import Link from "@docusaurus/Link";

The refresh and sign-out API for an **SSR** client, where the refresh token
lives in an httpOnly cookie that JS can't read.

Refreshing goes through an HTTP request to the SSR host, which carries the
refresh token in the cookie and replies with a
[SlimTokenBundle](../../ssr/index.md#slimtokenbundle) containing a new access
token, or `null` when the session is gone.

## Properties

| Property                                               | Type                                                                                 |
| ------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| <Link id="property-refreshsession" /> `refreshSession` | () => `Promise`\<[`SlimTokenBundle`](../../ssr/index.md#slimtokenbundle) \| `null`\> |
| <Link id="property-signout" /> `signOut`               | () => `Promise`\<`void`\>                                                            |
