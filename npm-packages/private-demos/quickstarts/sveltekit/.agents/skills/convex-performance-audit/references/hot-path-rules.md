# Hot Path Rules

Use these rules when the top-level workflow points to read amplification, denormalization, index rollout, reactive query cost, or invalidation-heavy writes.

## Contents

- Core Principle
- Consistency Rule
- 1. Push Filters To Storage (indexes, migration rule, redundant indexes)
- 2. Minimize Data Sources (denormalization, fallback rule)
- 3. Minimize Row Size (digest tables)
- 4. Skip No-Op Writes
- 5. Match Consistency To Read Patterns (high-read/low-write, high-read/high-write)
- Convex-Specific Notes (reactive queries, point-in-time reads, triggers, aggregates, backfills)
- Verification

## Core Principle

Every byte read or written multiplies with concurrency.

Think:

`cost x calls_per_second x 86400`

In Convex, every write can also fan out into reactive invalidation, replication work, and downstream sync.

## Consistency Rule

If you fix a hot-path pattern for one function, audit sibling functions touching the same tables for the same pattern.

Do this especially for:

- multiple list queries over the same table
- multiple writers to the same table
- public browse and search queries over the same records
- helper functions reused by more than one endpoint

## 1. Push Filters To Storage

Both JavaScript `.filter()` and the Convex query `.filter()` method after a DB scan mean you already paid for the read. The Convex `.filter()` method has the same performance as filtering in JS, it does not push the predicate to the storage layer. Only `.withIndex()` and `.withSearchIndex()` actually reduce the documents scanned.

Prefer:

- `withIndex(...)`
- `.withSearchIndex(...)` for text search
- narrower tables
- summary tables

before accepting a scan-plus-filter pattern.

```ts
// Bad: scans then filters in JavaScript
export const listOpen = query({
  args: {},
  handler: async (ctx) => {
    const tasks = await ctx.db.query("tasks").collect();
    return tasks.filter((task) => task.status === "open");
  },
});
```

```ts
// Also bad: Convex .filter() does not push to storage either
export const listOpen = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db
      .query("tasks")
      .filter((q) => q.eq(q.field("status"), "open"))
      .collect();
  },
});
```

```ts
// Good: use an index so storage does the filtering
export const listOpen = query({
  args: {},
  handler: async (ctx) => {
    return await ctx.db
      .query("tasks")
      .withIndex("by_status", (q) => q.eq("status", "open"))
      .collect();
  },
});
```

### Migration rule for indexes

New indexes on partially backfilled fields can create correctness bugs during rollout.

Important Convex detail:

`undefined !== false`

If an older document is missing a field entirely, it will not match a compound index entry that expects `false`.

Do not trust old comments saying a field is "not backfilled" or "already backfilled". Verify.

If correctness depends on handling old and new states during rollout, do not improvise a partial-backfill workaround in the hot path. Use a migration-safe rollout and consult `skills/convex-migration-helper/SKILL.md`.

```ts
// Bad: optional booleans can miss older rows where the field is undefined
const projects = await ctx.db
  .query("projects")
  .withIndex("by_archived_and_updated", (q) => q.eq("isArchived", false))
  .order("desc")
  .take(20);
```

```ts
// Good: switch hot-path reads only after the rollout is migration-safe
// See the migration helper skill for dual-read / backfill / cutover patterns.
```

### Check for redundant indexes

Indexes like `by_foo` and `by_foo_and_bar` are usually redundant. You only need `by_foo_and_bar`, since you can query it with just the `foo` condition and omit `bar`. Extra indexes add storage cost and write overhead on every insert, patch, and delete.

```ts
// Bad: two indexes where one would do
defineTable({ team: v.id("teams"), user: v.id("users") })
  .index("by_team", ["team"])
  .index("by_team_and_user", ["team", "user"]);
```

```ts
// Good: single compound index serves both query patterns
defineTable({ team: v.id("teams"), user: v.id("users") }).index(
  "by_team_and_user",
  ["team", "user"],
);
```

Exception: `.index("by_foo", ["foo"])` is really an index on `foo` + `_creationTime`, while `.index("by_foo_and_bar", ["foo", "bar"])` is on `foo` + `bar` + `_creationTime`. If you need results sorted by `foo` then `_creationTime`, you need the single-field index because the compound one would sort by `bar` first.

## 2. Minimize Data Sources

Trace every read.

If a function resolves a foreign key for a tiny display field and a denormalized copy already exists, prefer the denormalized field on the hot path.

### When to denormalize

Denormalize when all of these are true:

- the path is hot
- the joined document is much larger than the field you need
- many readers are paying that join cost repeatedly

Useful mental model:

`join_cost = rows_per_page x foreign_doc_size x pages_per_second`

Small-table joins are often fine. Large-document joins for tiny fields on hot list pages are usually not.

### Fallback rule

Denormalized data is an optimization. Live data is the correctness path.

Rules:

- If the denormalized field is missing or null, fall back to the live read
- Do not show placeholders instead of falling back
- In lookup maps, only include fully populated entries

```ts
// Bad: missing denormalized data becomes a placeholder and blocks correctness
const ownerName = project.ownerName ?? "Unknown owner";
```

```ts
// Good: denormalized data is an optimization, not the only source of truth
const ownerName =
  project.ownerName ?? (await ctx.db.get(project.ownerId))?.name ?? null;
```

Bad lookup map pattern:

```ts
const ownersById = {
  [project.ownerId]: { ownerName: null },
};
```

That blocks fallback because the map says "I have data" when it does not.

Good lookup map pattern:

```ts
const ownersById =
  project.ownerName !== undefined && project.ownerName !== null
    ? { [project.ownerId]: { ownerName: project.ownerName } }
    : {};
```

### No denormalized copy yet

Prefer adding fields to an existing summary, companion, or digest table instead of bloating the primary hot-path table.

If introducing the new field or table requires a staged rollout, backfill, or old/new-shape handling, use the migration helper skill for the rollout plan.

Rollout order:

1. Update schema
2. Update write path
3. Backfill
4. Switch read path

## 3. Minimize Row Size

Hot list pages should read the smallest document shape that still answers the UI.

Prefer summary or digest tables over full source tables when:

- the list page only needs a subset of fields
- source documents are large
- the query is high volume

An 800 byte summary row is materially cheaper than a 3 KB full document on a hot page.

Digest tables are a tradeoff, not a default:

- Worth it when the path is clearly hot, the source rows are much larger than the UI needs, or many readers are repeatedly paying the same join and payload cost
- Probably not worth it when an indexed read on the source table is already cheap enough, the table is still small, or the extra write and migration complexity would dominate the benefit

```ts
// Bad: list page reads source docs, then joins owner data per row
const projects = await ctx.db
  .query("projects")
  .withIndex("by_public", (q) => q.eq("isPublic", true))
  .collect();
```

```ts
// Good: list page reads the smaller digest shape first
const projects = await ctx.db
  .query("projectDigests")
  .withIndex("by_public_and_updated", (q) => q.eq("isPublic", true))
  .order("desc")
  .take(20);
```

## 4. Skip No-Op Writes

No-op writes still cost work in Convex:

- invalidation
- replication
- trigger execution
- downstream sync

Before `patch` or `replace`, compare against the existing document and skip the write if nothing changed.

Apply this across sibling writers too. One careful writer does not help much if three other mutations still patch unconditionally.

```ts
// Bad: patching unchanged values still triggers invalidation and downstream work
await ctx.db.patch(settings._id, {
  theme: args.theme,
  locale: args.locale,
});
```

```ts
// Good: only write when something actually changed
if (settings.theme !== args.theme || settings.locale !== args.locale) {
  await ctx.db.patch(settings._id, {
    theme: args.theme,
    locale: args.locale,
  });
}
```

## 5. Match Consistency To Read Patterns

Choose read strategy based on traffic shape.

### High-read, low-write

Examples:

- public browse pages
- search results
- landing pages
- directory listings

Prefer:

- point-in-time reads where appropriate
- explicit refresh
- local state for pagination
- caching where appropriate

Do not treat subscriptions as automatically wrong here. Prefer point-in-time reads only when the product does not need live freshness and the reactive cost is material. See `subscription-cost.md` for detailed patterns.

### High-read, high-write

Examples:

- collaborative editors
- live dashboards
- presence-heavy views

Reactive queries may be worth the ongoing cost.

## Convex-Specific Notes

### Reactive queries

Every `ctx.db.get()` and `ctx.db.query()` contributes to the invalidation set for the query.

On the client:

- `useQuery` creates a live subscription
- `usePaginatedQuery` creates a live subscription per page

For low-freshness flows, consider a point-in-time read instead of a live subscription only when the product does not need updates pushed automatically.

### Point-in-time reads

Framework helpers, server-rendered fetches, or one-shot client reads can avoid ongoing subscription cost when live updates are not useful.

Use them for:

- aggregate snapshots
- reports
- low-churn listings
- pages where explicit refresh is fine

### Triggers and fan-out

Triggers fire on every write, including writes that did not materially change the document.

When a write exists only to keep derived state in sync:

- diff before patching
- move expensive non-blocking work to `ctx.scheduler.runAfter` when appropriate

### Aggregates

Reactive global counts invalidate frequently on busy tables.

Prefer:

- one-shot aggregate fetches
- periodic recomputation
- precomputed summary rows

for global stats that do not need live updates every second.

### Backfills

For larger backfills, use cursor-based, self-scheduling `internalMutation` jobs or the migrations component.

Deploy code that can handle both states before running the backfill.

During the gap:

- writes should populate the new shape
- reads should fall back safely

## Verification

Before closing the audit, confirm:

1. Same results as before, no dropped records
2. The removed table or lookup is no longer in the hot-path read set
3. Tests or validation cover fallback behavior
4. Migration safety is preserved while fields or indexes are unbackfilled
5. Sibling functions were fixed consistently
