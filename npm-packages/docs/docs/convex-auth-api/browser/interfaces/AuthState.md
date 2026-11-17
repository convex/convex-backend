---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: AuthState
---

import Link from "@docusaurus/Link";

Auth state that can be subscribed to via
[AuthClient.subscribe](../classes/AuthClient.md#subscribe)

## Properties

| Property                                                 | Type               |
| -------------------------------------------------------- | ------------------ |
| <Link id="property-isauthenticated" /> `isAuthenticated` | `boolean`          |
| <Link id="property-isloading" /> `isLoading`             | `boolean`          |
| <Link id="property-token" /> `token`                     | `string` \| `null` |
