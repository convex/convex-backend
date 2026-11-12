---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: SpaAuthApi
---

import Link from "@docusaurus/Link";

The refresh and sign-out API for a **SPA** client, where JS holds the refresh
token directly.

Refreshing resolves to a [RefreshResult](../../ssr/index.md#refreshresult):
`rotated` carries the next refresh token to persist, and `noSession` means the
session is gone.

## Properties

| Property                                               | Type                                                                                                                                                                                                                       |
| ------------------------------------------------------ | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-refreshsession" /> `refreshSession` | (`refreshToken`) => `Promise`\< \| \{ `kind`: `"rotated"`; `tokens`: \{ `accessToken`: `string`; `refreshToken`: `string`; `refreshTokenExpiresAt`: `number`; `userId`: `string`; \}; \} \| \{ `kind`: `"noSession"`; \}\> |
| <Link id="property-signout" /> `signOut`               | (`refreshToken`) => `Promise`\<`void`\>                                                                                                                                                                                    |
