# 3.0.0

- Sync using the `/api/v1/data/sync` API instead of `list_snapshot` and
  `document_deltas`. Existing connections migrate automatically and keep their
  data — no historical resync is required.
- Changing the table, schema, or component selection now applies retroactively:
  newly selected tables are backfilled without a historical resync. Deselected
  tables keep their synced data in the destination and stop updating. Reselected
  tables are truncated and backfilled.
- Requires a Convex backend that serves `/api/v1/data/sync`. Self-hosted
  deployments older than that must be upgraded.

# 2.0.2

- Report sync errors to Fivetran as tasks instead of gRPC errors, so that error
  messages are shown to customers on the Fivetran dashboard.

# 2.0.1

- Improve error messages for failed API calls.

# 2.0.0

- Add support for Convex components.
- Add support for partial component, table, and column selection.
- This version requires a full historical resync for existing connections.

# 0.6.0

- Update rust nightly version in .rust-toolchain
- Remove deprecated set/map support
- Add gzip support to connector

# 0.5.0

- Emit a TRUNCATE op before the first write to any given table (in case it's a
  resync)
- Skip emitting an initial sync snapshot before the first page.

# 0.4.0

- Revert the `_convex_cursor` changes.
- Few updates to loglines
- docs updates

# 0.3.0

- Support a `_convex_cursor` table with a single column `cursor` which holds the
  `document_deltas` cursor representing the most recent sync.
- Add documentation to docs/

# 0.2.0

- Bump `convex` dep to 0.5.0
- Use /test_streaming_export_connection and /get_tables_and_columns endpoints
  rather than json_schemas json_schemas has stricter requirements around nested
  schemas than what fivetran requires.
