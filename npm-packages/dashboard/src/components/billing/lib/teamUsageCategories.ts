export const DATABASE_STORAGE_CATEGORIES = {
  document: {
    name: "Tables",
    color: "fill-chart-line-1",
  },
  index: {
    name: "Indexes",
    color: "fill-chart-line-2",
    tip: "Each index you define is billed as a copy of the table's documents.",
  },
};

export const BANDWIDTH_CATEGORIES = {
  egress: {
    name: "Reads",
    color: "fill-chart-line-1",
    backgroundColor: "bg-chart-line-1/30 dark:bg-chart-line-1",
  },
  ingress: {
    name: "Writes",
    color: "fill-chart-line-2",
    backgroundColor: "bg-chart-line-2/30 dark:bg-chart-line-2",
  },
};

export const TAG_CATEGORIES = {
  query: {
    name: "Queries",
    color: "fill-chart-line-1",
  },
  mutation: {
    name: "Mutations",
    color: "fill-chart-line-2",
  },
  action: {
    name: "Actions",
    color: "fill-chart-line-3",
  },
  http_action: {
    name: "HTTP Actions",
    color: "fill-chart-line-4",
  },
  storage_api: {
    name: "Storage API",
    color: "fill-chart-line-5",
  },
};

export const DATA_EGRESS_CATEGORIES = {
  fetchEgress: {
    name: "Fetch Egress",
    color: "fill-chart-line-1",
  },
  logStream: {
    name: "Log Streams",
    color: "fill-chart-line-2",
  },
  streamingExport: {
    name: "Streaming Export",
    color: "fill-chart-line-6",
  },
  servingEgress: {
    name: "File Serving",
    color: "fill-chart-line-3",
  },
  userFunctionEgress: {
    name: "User Function Reads",
    color: "fill-chart-line-4",
  },
  backupRestore: {
    name: "Backup / Restore",
    color: "fill-chart-line-5",
  },
};

export const DATA_EGRESS_CATEGORY_RENAMES: Record<string, string> = {
  backup: "backupRestore",
  restore: "backupRestore",
};

export const SEARCH_STORAGE_CATEGORIES = {
  textSearch: {
    name: "Text Search",
    color: "fill-chart-line-1",
  },
  vector: {
    name: "Vector",
    color: "fill-chart-line-2",
  },
};

export const SEARCH_QUERIES_CATEGORIES = {
  textSearch: {
    name: "Text Search",
    color: "fill-chart-line-1",
  },
  vectorSearch: {
    name: "Vector Search",
    color: "fill-chart-line-2",
  },
};

export const DATABASE_IO_CATEGORIES = {
  egress: {
    name: "Reads",
    color: "fill-chart-line-1",
  },
  ingress: {
    name: "Writes",
    color: "fill-chart-line-2",
  },
};

export const COMPUTE_CATEGORIES_SELF_SERVE = {
  actionConvex: {
    name: "Action",
    color: "fill-chart-line-1",
  },
  actionNode: {
    name: "Action (Node)",
    color: "fill-chart-line-2",
  },
};

export const COMPUTE_CATEGORIES = {
  queryMutation: {
    name: "Query/Mutation (Dedicated)",
    color: "fill-chart-line-1",
  },
  actionConvex: {
    name: "Action",
    color: "fill-chart-line-2",
  },
  actionNode: {
    name: "Action (Node)",
    color: "fill-chart-line-3",
  },
};

const DEPLOYMENT_CLASS_COLORS: Record<string, string> = {
  s16: "fill-chart-line-1",
  s256: "fill-chart-line-2",
  d1024: "fill-chart-line-3",
  d2048: "fill-chart-line-4",
};

export const DEPLOYMENT_CLASS_CATEGORIES: Record<
  string,
  { name: string; color: string }
> = Object.fromEntries(
  Object.entries(DEPLOYMENT_CLASS_COLORS).map(([key, color]) => [
    key,
    { name: key.toUpperCase(), color },
  ]),
);

export const DEPLOYMENT_STATUS_CATEGORIES: Record<
  string,
  { name: string; color: string; tip?: string }
> = {
  active: { name: "Active", color: "fill-chart-line-1" },
  idle: {
    name: "Idle",
    color: "fill-chart-line-8",
    tip: "Ran no functions in the last 30 days.",
  },
  paused: { name: "Paused", color: "fill-chart-line-4" },
};

export const CATEGORY_RENAMES = {
  uncached_query: "query",
  cached_query: "query",
};
