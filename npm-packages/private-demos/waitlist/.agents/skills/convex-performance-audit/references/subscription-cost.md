# Subscription Cost

Use these rules when the problem is too many reactive subscriptions, queries
invalidating too frequently, or React components re-rendering excessively due to
Convex state changes.

## Core Principle

Every `useQuery` and `usePaginatedQuery` call creates a live subscription. The
server tracks the query's read set and re-executes the query whenever any
document in that read set changes. Subscription cost scales with:

`subscriptions x invalidation_frequency x query_cost`

Subscriptions are not inherently bad. Convex reactivity is often the right
default. The goal is to reduce unnecessary invalidation work, not to eliminate
subscriptions on principle.

## Symptoms

- Dashboard shows high active subscription count
- UI feels sluggish or laggy despite fast individual queries
- React profiling shows frequent re-renders from Convex state
- Pages with many components each running their own `useQuery`
- Paginated lists where every loaded page stays subscribed

## Common Causes

### Reactive queries on low-freshness flows

Some user flows are read-heavy and do not need live updates every time the
underlying data changes. In those cases, ongoing subscriptions may cost more
than they are worth.

### Overly broad queries

A query that returns a large result set invalidates whenever any document in
that set changes. The broader the query, the more frequent the invalidation.

### Too many subscriptions per page

A page with 20 list items, each running its own `useQuery` to fetch related
data, creates 20+ subscriptions per visitor.

### Paginated queries keeping all pages live

`usePaginatedQuery` with `loadMore` keeps every loaded page subscribed. On a
page where a user has scrolled through 10 pages, all 10 stay reactive.

### Frequently-updated fields on widely-read documents

A document that many queries touch gets a frequently-updated field (like
`lastSeen`, `lastActiveAt`, or a counter). Every write to that field invalidates
every subscription that reads the document, even if those subscriptions never
use the field. This is different from OCC conflicts (see `occ-conflicts.md`),
which are write-vs-write contention. This is write-vs-subscription: the write
succeeds fine, but it forces hundreds of queries to re-run for no reason.

## Fix Order

### 1. Use point-in-time reads when live updates are not valuable

Keep `useQuery` and `usePaginatedQuery` by default when the product benefits
from fresh live data.

Consider a point-in-time read instead when all of these are true:

- the flow is high-read
- the underlying data changes less often than users need to see
- explicit refresh, periodic refresh, or a fresh read on navigation is
  acceptable

Possible implementations depend on environment:

- a server-rendered fetch
- a framework helper like `fetchQuery`
- a point-in-time client read such as `ConvexHttpClient.query()`

```ts
// Reactive by default when fresh live data matters
function TeamPresence() {
  const presence = useQuery(api.teams.livePresence, { teamId });
  return <PresenceList users={presence} />;
}
```

```ts
// Point-in-time read when explicit refresh is acceptable
import { ConvexHttpClient } from "convex/browser";

const client = new ConvexHttpClient(import.meta.env.VITE_CONVEX_URL);

function SnapshotView() {
  const [items, setItems] = useState<Item[]>([]);

  useEffect(() => {
    client.query(api.items.snapshot).then(setItems);
  }, []);

  return <ItemGrid items={items} />;
}
```

Good candidates for point-in-time reads:

- aggregate snapshots
- reports
- low-churn listings
- flows where explicit refresh is already acceptable

Keep reactive for:

- collaborative editing
- live dashboards
- presence-heavy views
- any surface where users expect fresh changes to appear automatically

### 2. Batch related data into fewer queries

Instead of N components each fetching their own related data, fetch it in a
single query.

```ts
// Bad: each card fetches its own author
function ProjectCard({ project }: { project: Project }) {
  const author = useQuery(api.users.get, { id: project.authorId });
  return <Card title={project.name} author={author?.name} />;
}
```

```ts
// Good: parent query returns projects with author names included
function ProjectList() {
  const projects = useQuery(api.projects.listWithAuthors);
  return projects?.map((p) => (
    <Card key={p._id} title={p.name} author={p.authorName} />
  ));
}
```

This can use denormalized fields or server-side joins in the query handler.
Either way, it is one subscription instead of N.

This is not automatically better. If the combined query becomes much broader and
invalidates much more often, several narrower subscriptions may be the better
tradeoff. Optimize for total invalidation cost, not raw subscription count.

### 3. Use skip to avoid unnecessary subscriptions

The `"skip"` value prevents a subscription from being created when the arguments
are not ready.

```ts
// Bad: subscribes with undefined args, wastes a subscription slot
const profile = useQuery(api.users.getProfile, { userId: selectedId! });
```

```ts
// Good: skip when there is nothing to fetch
const profile = useQuery(
  api.users.getProfile,
  selectedId ? { userId: selectedId } : "skip",
);
```

### 4. Isolate frequently-updated fields into separate documents

If a document is widely read but has a field that changes often, move that field
to a separate document. Queries that do not need the field will no longer be
invalidated by its writes.

```ts
// Bad: lastSeen lives on the user doc, every heartbeat invalidates
// every query that reads this user
const users = defineTable({
  name: v.string(),
  email: v.string(),
  lastSeen: v.number(),
});
```

```ts
// Good: lastSeen lives in a separate heartbeat doc
const users = defineTable({
  name: v.string(),
  email: v.string(),
  heartbeatId: v.id("heartbeats"),
});

const heartbeats = defineTable({
  lastSeen: v.number(),
});
```

Queries that only need `name` and `email` no longer re-run on every heartbeat.
Queries that actually need online status fetch the heartbeat document
explicitly.

For an even further optimization, if you only need a coarse online/offline
boolean rather than the exact `lastSeen` timestamp, add a separate presence
document with an `isOnline` flag. Update it immediately when a user comes
online, and use a cron to batch-mark users offline when their heartbeat goes
stale. This way the presence query only invalidates when online status actually
changes, not on every heartbeat.

### 5. Use the aggregate component for counts and sums

Reactive global counts (`SELECT COUNT(*)` equivalent) invalidate on every insert
or delete to the table. The
[`@convex-dev/aggregate`](https://www.npmjs.com/package/@convex-dev/aggregate)
component maintains denormalized COUNT, SUM, and MAX values efficiently so you
do not need a reactive query scanning the full table.

Use it for leaderboards, totals, "X items" badges, or any stat that would
otherwise require scanning many rows reactively.

If the aggregate component is not appropriate, prefer point-in-time reads for
global stats, or precomputed summary rows updated by a cron or trigger, over
reactive queries that scan large tables.

### 6. Narrow query read sets

Queries that return less data and touch fewer documents invalidate less often.

```ts
// Bad: returns all fields, invalidates on any field change
export const list = query({
  handler: async (ctx) => {
    return await ctx.db.query("projects").collect();
  },
});
```

```ts
// Good: use a digest table with only the fields the list needs
export const listDigests = query({
  handler: async (ctx) => {
    return await ctx.db.query("projectDigests").collect();
  },
});
```

Writes to fields not in the digest table do not invalidate the digest query.

### 7. Remove `Date.now()` from queries

Using `Date.now()` inside a query defeats Convex's query cache. The cache is
invalidated frequently to avoid showing stale time-dependent results, which
increases database work even when the underlying data has not changed.

```ts
// Bad: Date.now() defeats query caching and causes frequent re-evaluation
const releasedPosts = await ctx.db
  .query("posts")
  .withIndex("by_released_at", (q) => q.lte("releasedAt", Date.now()))
  .take(100);
```

```ts
// Good: use a boolean field updated by a scheduled function
const releasedPosts = await ctx.db
  .query("posts")
  .withIndex("by_is_released", (q) => q.eq("isReleased", true))
  .take(100);
```

If the query must compare against a time value, pass it as an explicit argument
from the client and round it to a coarse interval (e.g. the most recent minute)
so requests within that window share the same cache entry.

### 8. Consider pagination strategy

For long lists where users scroll through many pages:

- If the data does not need live updates, use point-in-time fetching with manual
  "load more"
- If it does need live updates, accept the subscription cost but limit the
  number of loaded pages
- Consider whether older pages can be unloaded as the user scrolls forward

### 9. Separate backend cost from UI churn

If the main problem is loading flash or UI churn when query arguments change,
stabilizing the reactive UI behavior may be better than replacing reactivity
altogether.

Treat this as a UX problem first when:

- the underlying query is already reasonably cheap
- the complaint is flicker, loading flashes, or re-render churn
- live updates are still desirable once fresh data arrives

## Verification

1. Subscription count in dashboard is lower for the affected pages
2. UI responsiveness has improved
3. React profiling shows fewer unnecessary re-renders
4. Surfaces that do not need live updates are not paying for persistent
   subscriptions unnecessarily
5. Sibling pages with similar patterns were updated consistently
