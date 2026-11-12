---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: CookieOptions
---

import Link from "@docusaurus/Link";

Attributes applied when writing a cookie. The low-level shape, all optional.

## Properties

| Property                                    | Type                              | Description                                |
| ------------------------------------------- | --------------------------------- | ------------------------------------------ |
| <Link id="property-domain" /> `domain?`     | `string`                          | Cookie domain.                             |
| <Link id="property-expires" /> `expires?`   | `Date`                            | Absolute expiry.                           |
| <Link id="property-httponly" /> `httpOnly?` | `boolean`                         | Whether to hide the cookie from client JS. |
| <Link id="property-maxage" /> `maxAge?`     | `number`                          | Max age in seconds.                        |
| <Link id="property-path" /> `path?`         | `string`                          | Cookie path. Defaults to `"/"`.            |
| <Link id="property-samesite" /> `sameSite?` | `"none"` \| `"lax"` \| `"strict"` | CSRF posture. Defaults to `"lax"`.         |
| <Link id="property-secure" /> `secure?`     | `boolean`                         | Whether to send only over HTTPS.           |
