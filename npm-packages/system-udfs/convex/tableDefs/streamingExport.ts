import { v } from "convex/values";

const inclusionDefault = v.union(v.literal("included"), v.literal("excluded"));

// The components, tables, and columns mirrored by an Analytics export, in the
// streaming export `Selection` wire format: nested maps keyed by component,
// then table, then column, where `_other` gives the default for entries that
// aren't listed.
export const syncSelection = v.record(
  v.string(),
  v.union(
    inclusionDefault,
    v.record(
      v.string(),
      v.union(inclusionDefault, v.record(v.string(), inclusionDefault)),
    ),
  ),
);

export const syncPeriod = v.union(
  v.literal("continuous"),
  v.literal("hourly"),
  v.literal("daily"),
);
