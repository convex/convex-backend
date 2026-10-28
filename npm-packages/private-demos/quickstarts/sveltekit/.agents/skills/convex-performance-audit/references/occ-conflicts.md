# OCC Conflict Resolution

Use these rules when insights, logs, or dashboard health show OCC (Optimistic Concurrency Control) conflicts, mutation retries, or write contention on hot tables.

## Core Principle

Convex uses optimistic concurrency control. When two transactions read or write overlapping data, one succeeds and the other retries automatically. High contention means wasted work and increased latency.

## Symptoms

- OCC conflict errors in deployment logs or health page
- Mutations retrying multiple times before succeeding
- User-visible latency spikes on write-heavy pages
- `npx convex insights --details` showing high conflict rates

## Common Causes

### Hot documents

Multiple mutations writing to the same document concurrently. Classic examples: a global counter, a shared settings row, or a "last updated" timestamp on a parent record.

### Broad read sets causing false conflicts

A query that scans a large table range creates a broad read set. If any write touches that range, the query's transaction conflicts even if the specific document the query cared about was not modified.

### Fan-out from triggers or cascading writes

A single user action triggers multiple mutations that all touch related documents. Each mutation competes with the others.

Database triggers (e.g. from `convex-helpers`) run inside the same transaction as the mutation that caused them. If a trigger does heavy work, reads extra tables, or writes to many documents, it extends the transaction's read/write set and increases the window for conflicts. Keep trigger logic minimal, or move expensive derived work to a scheduled function.

### Write-then-read chains

A mutation writes a document, then a reactive query re-reads it, then another mutation writes it again. Under load, these chains stack up.

## Fix Order

### 1. Reduce read set size

Narrower reads mean fewer false conflicts.

```ts
// Bad: broad scan creates a wide conflict surface
const allTasks = await ctx.db.query("tasks").collect();
const mine = allTasks.filter((t) => t.ownerId === userId);
```

```ts
// Good: indexed query touches only relevant documents
const mine = await ctx.db
  .query("tasks")
  .withIndex("by_owner", (q) => q.eq("ownerId", userId))
  .collect();
```

### 2. Split hot documents

When many writers target the same document, split the contention point.

```ts
// Bad: every vote increments the same counter document
const counter = await ctx.db.get(pollCounterId);
await ctx.db.patch(pollCounterId, { count: counter!.count + 1 });
```

```ts
// Good: shard the counter across multiple documents, aggregate on read
const shardIndex = Math.floor(Math.random() * SHARD_COUNT);
const shardId = shardIds[shardIndex];
const shard = await ctx.db.get(shardId);
await ctx.db.patch(shardId, { count: shard!.count + 1 });
```

Aggregate the shards in a query or scheduled job when you need the total.

### 3. Skip no-op writes

Writes that do not change data still participate in conflict detection and trigger invalidation.

```ts
// Bad: patches even when nothing changed
await ctx.db.patch(doc._id, { status: args.status });
```

```ts
// Good: only write when the value actually differs
if (doc.status !== args.status) {
  await ctx.db.patch(doc._id, { status: args.status });
}
```

### 4. Move non-critical work to scheduled functions

If a mutation does primary work plus secondary bookkeeping (analytics, notifications, cache warming), the bookkeeping extends the transaction's lifetime and read/write set.

```ts
// Bad: analytics update in the same transaction as the user action
await ctx.db.patch(userId, { lastActiveAt: Date.now() });
await ctx.db.insert("analytics", { event: "action", userId, ts: Date.now() });
```

```ts
// Good: schedule the bookkeeping so the primary transaction is smaller
await ctx.db.patch(userId, { lastActiveAt: Date.now() });
await ctx.scheduler.runAfter(0, internal.analytics.recordEvent, {
  event: "action",
  userId,
});
```

### 5. Combine competing writes

If two mutations must update the same document atomically, consider whether they can be combined into a single mutation call from the client, reducing round trips and conflict windows.

Do not introduce artificial locks or queues unless the above steps have been tried first.

## Related: Invalidation Scope

Splitting hot documents also reduces subscription invalidation, not just OCC contention. If a document is written frequently and read by many queries, those queries re-run on every write even when the fields they care about have not changed. See `subscription-cost.md` section 4 ("Isolate frequently-updated fields") for that pattern.

## Verification

1. OCC conflict rate has dropped in insights or dashboard
2. Mutation latency is lower and more consistent
3. No data correctness regressions from splitting or scheduling changes
4. Sibling writers to the same hot documents were fixed consistently
