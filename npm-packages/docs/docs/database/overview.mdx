---
title: "Database"
description: "Store JSON-like documents with a relational data model"
hide_table_of_contents: true
sidebar_label: Overview
sidebar_position: 0
pagination_next: database/reading-data/reading-data
---

The Convex database provides a relational data model, stores JSON-like
documents, and can be used with or without a schema. It "just works," giving you
predictable query performance in an easy-to-use interface.

Query and mutation [functions](/functions/overview.mdx) read and write data
through a lightweight JavaScript API. There is nothing to set up and no need to
write any SQL. Just use JavaScript to express your app's needs.

Start by learning about the basics of [tables](#tables), [documents](#documents)
and [schemas](#schemas) below, then move on to
[Reading Data](/database/reading-data/reading-data.mdx) and
[Writing Data](/database/writing-data.mdx).

As your app grows more complex you'll need more from your database:

- Relational data modeling with [Document IDs](/database/document-ids.mdx)
- Fast querying with [Indexes](/database/reading-data/indexes/indexes.mdx)
- Exposing large datasets with [Paginated Queries](/database/pagination.mdx)
- Type safety by [Defining a Schema](/database/schemas.mdx)
- Interoperability with data
  [Import & Export](docs/database/import-export/import-export.mdx)

## Tables

Your Convex deployment contains tables that hold your app's data. Initially,
your deployment contains no tables or documents.

Each table springs into existence as soon as you add the first document to it.

```javascript
// `friends` table doesn't exist.
await ctx.db.insert("friends", { name: "Jamie" });
// Now it does, and it has one document.
```

You do not have to specify a schema upfront or create tables explicitly.

## Documents

Tables contain documents. Documents are very similar to JavaScript objects. They
have fields and values, and you can nest arrays or objects within them.

These are all valid Convex documents:

```json
{}
{"name": "Jamie"}
{"name": {"first": "Ari", "second": "Cole"}, "age": 60}
```

They can also contain references to other documents in other tables. See
[Data Types](/database/types.mdx) to learn more about the types supported in
Convex and [Document IDs](/database/document-ids.mdx) to learn about how to use
those types to model your data.

## Schemas

Though optional, schemas ensure that your data looks exactly how you want. For a
simple chat app, the schema could look like this:

```typescript
import { defineSchema, defineTable } from "convex/server";
import { v } from "convex/values";

export default defineSchema({
  messages: defineTable({
    author: v.id("users"),
    body: v.string(),
  }),
  users: defineTable({
    name: v.string(),
    userName: v.string(),
  }),
});
```

You can choose to be as flexible as you want by using types such as `v.any()` or
as specific as you want by precisely describing a `v.object()`.

See [the schema documentation](/database/schemas.mdx) to learn more about
schemas.

<CardLink
  className="convex-hero-card"
  item={{
    href: "/database/reading-data",
    docId: "database/reading-data/reading-data",
    label: "Next: Reading Data",
  }}
/>

<StackPosts query="database" />
