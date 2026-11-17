---
# NOTE: imported from https://github.com/convex/convex-auth by
# scripts/import-convex-auth-docs.js. Edit the source in that repo and
# re-import with `just import-convex-auth-docs <tag>`. If you need to make
# changes here, make sure to upstream them to the source repo too or
# they'll be overwritten on the next import.
title: InMemoryStorage
---

An in-memory [TokenStorage](../../react/interfaces/TokenStorage.md), used as a
fallback when no persistent storage is available (SSR, or a non-browser
runtime). Sessions stored here do not survive a reload.

## Implements

- [`TokenStorage`](../../react/interfaces/TokenStorage.md)

## Constructors

### Constructor

```ts
new InMemoryStorage(): InMemoryStorage;
```

#### Returns

`InMemoryStorage`

## Methods

### getItem()

```ts
getItem(key): string;
```

Read a value.

#### Parameters

| Parameter | Type     |
| --------- | -------- |
| `key`     | `string` |

#### Returns

`string`

#### Implementation of

[`TokenStorage`](../../react/interfaces/TokenStorage.md).[`getItem`](../../react/interfaces/TokenStorage.md#property-getitem)

---

### removeItem()

```ts
removeItem(key): void;
```

Remove a value.

#### Parameters

| Parameter | Type     |
| --------- | -------- |
| `key`     | `string` |

#### Returns

`void`

#### Implementation of

[`TokenStorage`](../../react/interfaces/TokenStorage.md).[`removeItem`](../../react/interfaces/TokenStorage.md#property-removeitem)

---

### setItem()

```ts
setItem(key, value): void;
```

Write a value.

#### Parameters

| Parameter | Type     |
| --------- | -------- |
| `key`     | `string` |
| `value`   | `string` |

#### Returns

`void`

#### Implementation of

[`TokenStorage`](../../react/interfaces/TokenStorage.md).[`setItem`](../../react/interfaces/TokenStorage.md#property-setitem)
