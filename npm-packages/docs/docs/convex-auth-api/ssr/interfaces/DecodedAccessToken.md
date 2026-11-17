---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: DecodedAccessToken
---

import Link from "@docusaurus/Link";

The claims the server cares about from an access token.

## Properties

| Property                         | Type               | Description                                                             |
| -------------------------------- | ------------------ | ----------------------------------------------------------------------- |
| <Link id="property-exp" /> `exp` | `number` \| `null` | Expiry as a UNIX timestamp in seconds, or `null` if absent/unparseable. |
| <Link id="property-sub" /> `sub` | `string` \| `null` | The subject (app user id), or `null` if absent/unparseable.             |
