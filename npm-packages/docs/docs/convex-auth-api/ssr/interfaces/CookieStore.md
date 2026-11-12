---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: CookieStore
---

import Link from "@docusaurus/Link";

Read/write/delete cookies. Every method may be synchronous or return a promise,
so request/response cookie APIs that are async (e.g. `next/headers` `cookies()`)
work without ceremony.

## Extended by

- [`HttpCookies`](HttpCookies.md)

## Properties

| Property                               | Type                                                                        | Description                                                                                                          |
| -------------------------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- |
| <Link id="property-delete" /> `delete` | (`name`, `options?`) => `void` \| `Promise`\<`void`\>                       | Delete a cookie. Must be given the same `path`/`domain` the cookie was written with, or the deletion won't match it. |
| <Link id="property-get" /> `get`       | (`name`) => `string` \| `Promise`\<`string` \| `undefined`\> \| `undefined` | Read a cookie value.                                                                                                 |
| <Link id="property-set" /> `set`       | (`name`, `value`, `options?`) => `void` \| `Promise`\<`void`\>              | Write a cookie.                                                                                                      |
