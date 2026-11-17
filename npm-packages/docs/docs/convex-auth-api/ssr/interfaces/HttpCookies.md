---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: HttpCookies
---

import Link from "@docusaurus/Link";

A [CookieStore](CookieStore.md) that also knows how to write its buffered
mutations onto a response's headers. Build one per request; after driving a
[ServerAuthSession](../classes/ServerAuthSession.md) with it, call
[HttpCookies.applyTo](#property-applyto) on the response you return.

## Extends

- [`CookieStore`](CookieStore.md)

## Properties

| Property                                 | Type                                                                        | Description                                                                                                          | Inherited from                                                             |
| ---------------------------------------- | --------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------- | -------------------------------------------------------------------------- |
| <Link id="property-applyto" /> `applyTo` | (`headers`) => `void`                                                       | Append a `Set-Cookie` header for every write/delete made on this store.                                              | -                                                                          |
| <Link id="property-delete" /> `delete`   | (`name`, `options?`) => `void` \| `Promise`\<`void`\>                       | Delete a cookie. Must be given the same `path`/`domain` the cookie was written with, or the deletion won't match it. | [`CookieStore`](CookieStore.md).[`delete`](CookieStore.md#property-delete) |
| <Link id="property-get" /> `get`         | (`name`) => `string` \| `Promise`\<`string` \| `undefined`\> \| `undefined` | Read a cookie value.                                                                                                 | [`CookieStore`](CookieStore.md).[`get`](CookieStore.md#property-get)       |
| <Link id="property-set" /> `set`         | (`name`, `value`, `options?`) => `void` \| `Promise`\<`void`\>              | Write a cookie.                                                                                                      | [`CookieStore`](CookieStore.md).[`set`](CookieStore.md#property-set)       |
