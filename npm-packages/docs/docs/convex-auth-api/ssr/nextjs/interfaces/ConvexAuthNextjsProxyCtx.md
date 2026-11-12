---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ConvexAuthNextjsProxyCtx
---

import Link from "@docusaurus/Link";

Context handed to a proxy handler.

## Properties

| Property                                                 | Type                                  | Description                                          |
| -------------------------------------------------------- | ------------------------------------- | ---------------------------------------------------- |
| <Link id="property-gettoken" /> `getToken`               | () => `Promise`\<`string` \| `null`\> | The (possibly just-refreshed) access token, or null. |
| <Link id="property-isauthenticated" /> `isAuthenticated` | () => `Promise`\<`boolean`\>          | Whether the request carries a non-expired session.   |
