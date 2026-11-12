---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: ServerAuthCheckerConfig
---

import Link from "@docusaurus/Link";

Configuration for
[createServerAuthChecker](../index.md#createserverauthchecker).

## Properties

| Property                                                 | Type                                                 | Description                                                                                                        |
| -------------------------------------------------------- | ---------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ |
| <Link id="property-convexurl" /> `convexUrl`             | `string`                                             | The Convex deployment URL used server-side.                                                                        |
| <Link id="property-isauthenticated" /> `isAuthenticated` | [`IsAuthenticatedFn`](../index.md#isauthenticatedfn) | The app's `isAuthenticated` query reference. Called with the access token so the backend can verify its signature. |
