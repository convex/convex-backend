---
name: convex-performance-audit
description: Audits and optimizes Convex application performance across hot-path reads, write contention, subscription cost, and function limits. Use this skill when a Convex feature is slow or expensive, npx convex insights shows high bytes or documents read, OCC conflict errors or mutation retries appear, subscriptions or UI updates are costly, functions hit execution or transaction limits, or the user mentions performance, latency, read amplification, or invalidation problems in a Convex app.
---

# Convex Performance Audit

Diagnose and fix performance problems in Convex applications, one problem class at a time.

## When to Use

- A Convex page or feature feels slow or expensive
- `npx convex insights --details` reports high bytes read, documents read, or OCC conflicts
- Low-freshness read paths are using reactivity where point-in-time reads would do
- OCC conflict errors or excessive mutation retries
- High subscription count or slow UI updates
- Functions approaching execution or transaction limits
- The same performance pattern needs fixing across sibling functions

## When Not to Use

- Initial Convex setup, auth setup, or component extraction
- Pure schema migrations with no performance goal
- One-off micro-optimizations without a user-visible or deployment-visible problem

## Guardrails

- Prefer simpler code when scale is small, traffic is modest, or the available signals are weak
- Do not recommend digest tables, document splitting, fetch-strategy changes, or migration-heavy rollouts unless there is a measured signal, a clearly unbounded path, or a known hot read/write path
- In Convex, a simple scan on a small table is often acceptable. Do not invent structural work just because a pattern is not ideal at large scale

## First Step: Gather Signals

Start with the strongest signal available:

1. If deployment Health insights are already available from the user or the current context, treat them as a first-class source of performance signals.
2. If CLI insights are available, run `npx convex insights --details`. Use `--prod`, `--preview-name`, or `--deployment-name` when needed.
   - If the local repo's Convex CLI is too old to support `insights`, try `npx -y convex@latest insights --details` before giving up.
3. If the repo already uses `convex-doctor`, you may treat its findings as hints. Do not require it, and do not treat it as the source of truth.
4. If runtime signals are unavailable, audit from code anyway, but keep the guardrails above in mind. Lack of insights is not proof of health, but it is also not proof that a large refactor is warranted.

## Signal Routing

After gathering signals, identify the problem class and read the matching reference file.

| Signal                                                         | Reference                                 |
| -------------------------------------------------------------- | ----------------------------------------- |
| High bytes or documents read, JS filtering, unnecessary joins  | `references/hot-path-rules.md`            |
| OCC conflict errors, write contention, mutation retries        | `references/occ-conflicts.md`             |
| High subscription count, slow UI updates, excessive re-renders | `references/subscription-cost.md`         |
| Function timeouts, transaction size errors, large payloads     | `references/function-budget.md`           |
| General "it's slow" with no specific signal                    | Start with `references/hot-path-rules.md` |

Multiple problem classes can overlap. Read the most relevant reference first, then check the others if symptoms remain.

## Escalate Larger Fixes

If the likely fix is invasive, cross-cutting, or migration-heavy, stop and present options before editing.

Examples:

- introducing digest or summary tables across multiple flows
- splitting documents to isolate frequently-updated fields
- reworking pagination or fetch strategy across several screens
- switching to a new index or denormalized field that needs migration-safe rollout

When correctness depends on handling old and new states during a rollout, consult `skills/convex-migration-helper/SKILL.md` for the migration workflow.

## Workflow

### 1. Scope the problem

Pick one concrete user flow from the actual project. Look at the codebase, client pages, and API surface to find the flow that matches the symptom.

Write down:

- entrypoint functions
- client callsites using `useQuery`, `usePaginatedQuery`, or `useMutation`
- tables read
- tables written
- whether the path is high-read, high-write, or both

### 2. Trace the full read and write set

For each function in the path:

1. Trace every `ctx.db.get()` and `ctx.db.query()`
2. Trace every `ctx.db.patch()`, `ctx.db.replace()`, and `ctx.db.insert()`
3. Note foreign-key lookups, JS-side filtering, and full-document reads
4. Identify all sibling functions touching the same tables
5. Identify reactive stats, aggregates, or widgets rendered on the same page

In Convex, every extra read increases transaction work, and every write can invalidate reactive subscribers. Treat read amplification and invalidation amplification as first-class problems.

### 3. Apply fixes from the relevant reference

Read the reference file matching your problem class. Each reference includes specific patterns, code examples, and a recommended fix order.

Do not stop at the single function named by an insight. Trace sibling readers and writers touching the same tables.

### 4. Fix sibling functions together

When one function touching a table has a performance bug, audit sibling functions for the same pattern.

After finding one problem, inspect both sibling readers and sibling writers for the same table family, including companion digest or summary tables.

Examples:

- If one list query switches from full docs to a digest table, inspect the other list queries for that table
- If one mutation needs no-op write protection, inspect the other writers to the same table
- If one read path needs a migration-safe rollout for an unbackfilled field, inspect sibling reads for the same rollout risk

Do not leave one path fixed and another path on the old pattern unless there is a clear product reason.

### 5. Verify before finishing

Confirm all of these:

1. Results are the same as before, no dropped records
2. Eliminated reads or writes are no longer in the path where expected
3. Fallback behavior works when denormalized or indexed fields are missing
4. New writes avoid unnecessary invalidation when data is unchanged
5. Every relevant sibling reader and writer was inspected, not just the original function

## Reference Files

- `references/hot-path-rules.md` - Read amplification, invalidation, denormalization, indexes, digest tables
- `references/occ-conflicts.md` - Write contention, OCC resolution, hot document splitting
- `references/subscription-cost.md` - Reactive query cost, subscription granularity, point-in-time reads
- `references/function-budget.md` - Execution limits, transaction size, large documents, payload size

Also check the official [Convex Best Practices](https://docs.convex.dev/understanding/best-practices/) page for additional patterns covering argument validation, access control, and code organization that may surface during the audit.

## Checklist

- [ ] Gathered signals from insights, dashboard, or code audit
- [ ] Identified the problem class and read the matching reference
- [ ] Scoped one concrete user flow or function path
- [ ] Traced every read and write in that path
- [ ] Identified sibling functions touching the same tables
- [ ] Applied fixes from the reference, following the recommended fix order
- [ ] Fixed sibling functions consistently
- [ ] Verified behavior and confirmed no regressions
