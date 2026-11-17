---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: TokenStorage
---

import Link from "@docusaurus/Link";

Read/write/delete string values by key. Every method may be synchronous or
return a promise, so async stores (e.g. React Native secure storage) work
without ceremony.

## Properties

| Property                                       | Type                                                                                              | Description     |
| ---------------------------------------------- | ------------------------------------------------------------------------------------------------- | --------------- |
| <Link id="property-getitem" /> `getItem`       | (`key`) => \| `string` \| `Promise`\<`string` \| `null` \| `undefined`\> \| `null` \| `undefined` | Read a value.   |
| <Link id="property-removeitem" /> `removeItem` | (`key`) => `void` \| `Promise`\<`void`\>                                                          | Remove a value. |
| <Link id="property-setitem" /> `setItem`       | (`key`, `value`) => `void` \| `Promise`\<`void`\>                                                 | Write a value.  |
