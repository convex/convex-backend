---
name: convex-migration-helper
description: Plans and executes safe Convex schema and data migrations using the widen-migrate-narrow workflow and the @convex-dev/migrations component. Use this skill when a deployment fails schema validation, existing documents need backfilling, fields need adding or removing or changing type, tables need splitting or merging, or a zero-downtime migration strategy is needed. Also use when the user mentions breaking schema changes, multi-deploy rollouts, or data transformations on existing Convex tables.
---

# Convex Migration Helper

Safely migrate Convex schemas and data when making breaking changes.

## When to Use

- Adding new required fields to existing tables
- Changing field types or structure
- Splitting or merging tables
- Renaming or deleting fields
- Migrating from nested to relational data

## When Not to Use

- Greenfield schema with no existing data in production or dev
- Adding optional fields that do not need backfilling
- Adding new tables with no existing data to migrate
- Adding or removing indexes with no correctness concern
- Questions about Convex schema design without a migration need

## Key Concepts

### Schema Validation Drives the Workflow

Convex will not let you deploy a schema that does not match the data at rest. This is the fundamental constraint that shapes every migration:

- You cannot add a required field if existing documents don't have it
- You cannot change a field's type if existing documents have the old type
- You cannot remove a field from the schema if existing documents still have it

This means migrations follow a predictable pattern: **widen the schema, migrate the data, narrow the schema**.

### Online Migrations

Convex migrations run online, meaning the app continues serving requests while data is updated asynchronously in batches. During the migration window, your code must handle both old and new data formats.

### Prefer New Fields Over Changing Types

When changing the shape of data, create a new field rather than modifying an existing one. This makes the transition safer and easier to roll back.

### Don't Delete Data

Unless you are certain, prefer deprecating fields over deleting them. Mark the field as `v.optional` and add a code comment explaining it is deprecated and why it existed.

## Safe Changes (No Migration Needed)

### Adding Optional Field

```typescript
// Before
users: defineTable({
  name: v.string(),
});

// After - safe, new field is optional
users: defineTable({
  name: v.string(),
  bio: v.optional(v.string()),
});
```

### Adding New Table

```typescript
posts: defineTable({
  userId: v.id("users"),
  title: v.string(),
}).index("by_user", ["userId"]);
```

### Adding Index

```typescript
users: defineTable({
  name: v.string(),
  email: v.string(),
}).index("by_email", ["email"]);
```

## Breaking Changes: The Deployment Workflow

Every breaking migration follows the same multi-deploy pattern:

**Deploy 1 - Widen the schema:**

1. Update schema to allow both old and new formats (e.g., add optional new field)
2. Update code to handle both formats when reading
3. Update code to write the new format for new documents
4. Deploy

**Between deploys - Migrate data:**

5. Run migration to backfill existing documents
6. Verify all documents are migrated

**Deploy 2 - Narrow the schema:**

7. Update schema to require the new format only
8. Remove code that handles the old format
9. Deploy

## Using the Migrations Component

For any non-trivial migration, use the [`@convex-dev/migrations`](https://www.convex.dev/components/migrations) component. It handles batching, cursor-based pagination, state tracking, resume from failure, dry runs, and progress monitoring.

See `references/migrations-component.md` for installation, setup, defining and running migrations, dry runs, status monitoring, and configuration options.

## Common Migration Patterns

See `references/migration-patterns.md` for complete patterns with code examples covering:

- Adding a required field
- Deleting a field
- Changing a field type
- Splitting nested data into a separate table
- Cleaning up orphaned documents
- Zero-downtime strategies (dual write, dual read)
- Small table shortcut (single internalMutation without the component)
- Verifying a migration is complete

## Common Pitfalls

1. **Making a field required before migrating data**: Convex rejects the deploy because existing documents lack the field. Always widen the schema first.
2. **Using `.collect()` on large tables**: Hits transaction limits or causes timeouts. Use the migrations component for proper batched pagination. `.collect()` is only safe for tables you know are small.
3. **Not writing the new format before migrating**: Documents created during the migration window will be missed, leaving unmigrated data after the migration "completes."
4. **Skipping the dry run**: Use `dryRun: true` to validate migration logic before committing changes to production data. Catches bugs before they touch real documents.
5. **Deleting fields prematurely**: Prefer deprecating with `v.optional` and a comment. Only delete after you are confident the data is no longer needed and no code references it.
6. **Using crons for migration batches**: The migrations component handles batching via recursive scheduling internally. Crons require manual cleanup and an extra deploy to remove.

## Migration Checklist

- [ ] Identify the breaking change and plan the multi-deploy workflow
- [ ] Update schema to allow both old and new formats
- [ ] Update code to handle both formats when reading
- [ ] Update code to write the new format for new documents
- [ ] Deploy widened schema and updated code
- [ ] Define migration using the `@convex-dev/migrations` component
- [ ] Test with `dryRun: true`
- [ ] Run migration and monitor status
- [ ] Verify all documents are migrated
- [ ] Update schema to require new format only
- [ ] Clean up code that handled old format
- [ ] Deploy final schema and code
- [ ] Remove migration code once confirmed stable
