# Triggers

> [!WARNING] This is an internal test project only, not intended for production
> use. If you're looking for the triggers helper used in production, see
> [convex-helpers](https://github.com/convex/convex-helpers/blob/main/packages/convex-helpers/README.md#triggers).

This is a component built for build-on-components-day.

## What can you trigger

Run code whenever a table changes.

Example usages:

- Attach a custom index to a table, like a geospatial index or a BTree with
  aggregates.
- Log whenever the table changes.
- Detect unauthorized access with Row Level Security and throw an error.

## How to use

Copy installation pattern from `convex/triggers.ts`.

Copy usage pattern from `convex/messages.ts`.

Each triggered function runs within the same mutation, immediately after the
document write. If you want async triggers, you can use
`ctx.scheduler.runAfter(0, ...);`

All of your functions that modify the table must be wrapped in
`mutationWithTriggers` instead of `mutation`, in order to trigger the triggers.
Triggers do not run when the table changes through a plain `mutation` or through
the Convex Dashboard.

Triggers are called with arguments matching `triggerArgsValidator`, so you know
whether it was an insert, patch, replace, or delete. You are also given the
document ID and the full document contents before and after the write.

The usage of components allows the `oldDoc` and `newDoc` to be computed
atomically, unlike the `DatabaseWriter` wrapper from the Row Level Security
helpers.
