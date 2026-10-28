# Migrations Component Reference

Complete guide to the [`@convex-dev/migrations`](https://www.convex.dev/components/migrations) component for batched, resumable Convex data migrations.

## Installation

```bash
npm install @convex-dev/migrations
```

## Setup

```typescript
// convex/convex.config.ts
import { defineApp } from "convex/server";
import migrations from "@convex-dev/migrations/convex.config.js";

const app = defineApp();
app.use(migrations);
export default app;
```

```typescript
// convex/migrations.ts
import { Migrations } from "@convex-dev/migrations";
import { components } from "./_generated/api.js";
import { DataModel } from "./_generated/dataModel.js";

export const migrations = new Migrations<DataModel>(components.migrations);
export const run = migrations.runner();
```

The `DataModel` type parameter is optional but provides type safety for migration definitions.

## Define a Migration

The `migrateOne` function processes a single document. The component handles batching and pagination automatically.

```typescript
// convex/migrations.ts
export const addDefaultRole = migrations.define({
  table: "users",
  migrateOne: async (ctx, user) => {
    if (user.role === undefined) {
      await ctx.db.patch(user._id, { role: "user" });
    }
  },
});
```

Shorthand: if you return an object, it is applied as a patch automatically.

```typescript
export const clearDeprecatedField = migrations.define({
  table: "users",
  migrateOne: () => ({ legacyField: undefined }),
});
```

## Run a Migration

From the CLI:

```bash
# Define a one-off runner in convex/migrations.ts:
#   export const runIt = migrations.runner(internal.migrations.addDefaultRole);
npx convex run migrations:runIt

# Or use the general-purpose runner
npx convex run migrations:run '{"fn": "migrations:addDefaultRole"}'
```

Programmatically from another Convex function:

```typescript
await migrations.runOne(ctx, internal.migrations.addDefaultRole);
```

## Run Multiple Migrations in Order

```typescript
export const runAll = migrations.runner([
  internal.migrations.addDefaultRole,
  internal.migrations.clearDeprecatedField,
  internal.migrations.normalizeEmails,
]);
```

```bash
npx convex run migrations:runAll
```

If one fails, it stops and will not continue to the next. Call it again to retry from where it left off. Completed migrations are skipped automatically.

## Dry Run

Test a migration before committing changes:

```bash
npx convex run migrations:runIt '{"dryRun": true}'
```

This runs one batch and then rolls back, so you can see what it would do without changing any data.

## Check Migration Status

```bash
npx convex run --component migrations lib:getStatus --watch
```

## Cancel a Running Migration

```bash
npx convex run --component migrations lib:cancel '{"name": "migrations:addDefaultRole"}'
```

Or programmatically:

```typescript
await migrations.cancel(ctx, internal.migrations.addDefaultRole);
```

## Run Migrations on Deploy

Chain migration execution after deploying:

```bash
npx convex deploy --cmd 'npm run build' && npx convex run migrations:runAll --prod
```

## Configuration Options

### Custom Batch Size

If documents are large or the table has heavy write traffic, reduce the batch size to avoid transaction limits or OCC conflicts:

```typescript
export const migrateHeavyTable = migrations.define({
  table: "largeDocuments",
  batchSize: 10,
  migrateOne: async (ctx, doc) => {
    // migration logic
  },
});
```

### Migrate a Subset Using an Index

Process only matching documents instead of the full table:

```typescript
export const fixEmptyNames = migrations.define({
  table: "users",
  customRange: (query) => query.withIndex("by_name", (q) => q.eq("name", "")),
  migrateOne: () => ({ name: "<unknown>" }),
});
```

### Parallelize Within a Batch

By default each document in a batch is processed serially. Enable parallel processing if your migration logic does not depend on ordering:

```typescript
export const clearField = migrations.define({
  table: "myTable",
  parallelize: true,
  migrateOne: () => ({ optionalField: undefined }),
});
```
