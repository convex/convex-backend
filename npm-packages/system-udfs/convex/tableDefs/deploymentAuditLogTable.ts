import { syncSelection, syncPeriod } from "./streamingExport";
import { defineTable } from "convex/server";
import { GenericValidator, v } from "convex/values";
import {
  snapshotImportFormat,
  snapshotImportMode,
  snapshotImportRequestor,
} from "./snapshotImport";

const auditLogEventValidator = <
  const Action extends string,
  Metadata extends Record<string, GenericValidator>,
>(
  action: Action,
  metadata: Metadata,
) =>
  v.object({
    action: v.literal(action),
    member_id: v.union(v.int64(), v.null()),
    token_id: v.optional(v.union(v.int64(), v.null())),
    app_client_id: v.optional(v.union(v.string(), v.null())),
    client_ip: v.optional(v.union(v.string(), v.null())),
    client_user_agent: v.optional(v.union(v.string(), v.null())),
    metadata: v.object(metadata),
  });

const createEnvironmentVariable = auditLogEventValidator(
  "create_environment_variable",
  { variable_name: v.string() },
);

const deleteEnvironmentVariable = auditLogEventValidator(
  "delete_environment_variable",
  { variable_name: v.string() },
);

const updateEnvironmentVariable = auditLogEventValidator(
  "update_environment_variable",
  { variable_name: v.string() },
);

const replaceEnvironmentVariable = auditLogEventValidator(
  "replace_environment_variable",
  {
    previous_variable_name: v.string(),
    variable_name: v.string(),
  },
);

// The serialized shape of a deployment usage limit's configuration, written by
// the backend's `SerializedUsageLimitConfig` (serde/strum `camelCase`). `metric`,
// `window`, and `limitType` are the string forms of the backend enums; `limit`
// is a count of the metric's raw unit, stored as an int64.
export const usageLimitConfig = v.object({
  name: v.optional(v.union(v.string(), v.null())),
  metric: v.string(),
  window: v.string(),
  limitType: v.string(),
  limit: v.int64(),
  enabled: v.boolean(),
});

const createUsageLimit = auditLogEventValidator("create_usage_limit", {
  id: v.string(),
  config: usageLimitConfig,
});

const updateUsageLimit = auditLogEventValidator("update_usage_limit", {
  id: v.string(),
  previous: usageLimitConfig,
  current: usageLimitConfig,
});

const deleteUsageLimit = auditLogEventValidator("delete_usage_limit", {
  id: v.string(),
  config: usageLimitConfig,
});

const usageLimitExceeded = auditLogEventValidator("usage_limit_exceeded", {
  id: v.string(),
  config: usageLimitConfig,
});

export const usageLimitStopState = v.union(
  v.literal("none"),
  v.literal("disabled"),
);

const changeUsageLimitStopState = auditLogEventValidator(
  "change_usage_limit_stop_state",
  {
    old_state: usageLimitStopState,
    new_state: usageLimitStopState,
  },
);

const updateCanonicalUrl = auditLogEventValidator("update_canonical_url", {
  request_destination: v.string(),
  url: v.string(),
});

const deleteCanonicalUrl = auditLogEventValidator("delete_canonical_url", {
  request_destination: v.string(),
});

const databaseIndex = v.object({
  name: v.optional(v.string()),
  type: v.literal("database"),
  fields: v.array(v.string()),
});

const searchIndex = v.object({
  name: v.optional(v.string()),
  type: v.literal("search"),
  searchField: v.string(),
  filterFields: v.array(v.string()),
});

const vectorIndex = v.object({
  name: v.optional(v.string()),
  type: v.literal("vector"),
  vectorField: v.string(),
  filterFields: v.array(v.string()),
  dimensions: v.number(),
});

export const indexMetadata = v.union(databaseIndex, searchIndex, vectorIndex);

const indexConfigs = v.array(v.union(databaseIndex, searchIndex, vectorIndex));
export const buildIndexes = auditLogEventValidator("build_indexes", {
  added_indexes: indexConfigs,
  removed_indexes: indexConfigs,
});

export const indexDiff = v.object({
  added_indexes: indexConfigs,
  removed_indexes: indexConfigs,
});

export const authDiff = v.object({
  added: v.array(v.string()),
  removed: v.array(v.string()),
});

const serverVersion = v.union(
  v.null(),
  v.object({
    previous_version: v.string(),
    next_version: v.string(),
  }),
);
const nodeVersionDiff = v.union(
  v.null(),
  v.object({
    previous_version: v.union(v.string(), v.null()),
    next_version: v.union(v.string(), v.null()),
  }),
);
const moduleDiff = v.object({
  added: v.array(v.string()),
  removed: v.array(v.string()),
});
export const cronDiffType = v.optional(
  v.object({
    added: v.array(v.string()),
    updated: v.array(v.string()),
    deleted: v.array(v.string()),
  }),
);
export const schemaDiffType = v.optional(
  v.union(
    v.null(),
    v.object({
      previous_schema_id: v.union(v.id("_schemas"), v.null()),
      next_schema_id: v.union(v.id("_schemas"), v.null()),
      previous_schema: v.optional(v.union(v.string(), v.null())),
      next_schema: v.optional(v.union(v.string(), v.null())),
    }),
  ),
);

export const pushConfig = auditLogEventValidator("push_config", {
  auth: authDiff,
  server_version: serverVersion,
  modules: moduleDiff,
  crons: cronDiffType,
  schema: schemaDiffType,
});

export const componentDiff = v.object({
  diffType: v.object({
    type: v.union(
      v.literal("create"),
      v.literal("modify"),
      v.literal("unmount"),
      v.literal("remount"),
    ),
  }),
  indexDiff: v.optional(indexDiff),
  udfConfigDiff: serverVersion,
  moduleDiff: moduleDiff,
  cronDiff: cronDiffType,
  schemaDiff: schemaDiffType,
});

export const pushConfigWithComponents = auditLogEventValidator(
  "push_config_with_components",
  {
    auth_diff: v.optional(authDiff),
    component_diffs: v.array(
      v.object({
        component_path: v.union(v.string(), v.null()),
        component_diff: componentDiff,
      }),
    ),
    message: v.optional(v.string()),
    node_version_diff: v.optional(nodeVersionDiff),
  },
);

export const oldBackendState = v.union(
  v.literal("paused"),
  v.literal("running"),
  v.literal("disabled"),
  v.literal("suspended"),
);

export const changeDeploymentState = auditLogEventValidator(
  "change_deployment_state",
  {
    old_state: oldBackendState,
    new_state: oldBackendState,
  },
);

export const pauseDeployment = auditLogEventValidator("pause_deployment", {});

export const unpauseDeployment = auditLogEventValidator(
  "unpause_deployment",
  {},
);

export const systemStopState = v.union(
  v.literal("none"),
  v.literal("disabled"),
  v.literal("suspended"),
);

export const changeSystemStopState = auditLogEventValidator(
  "change_system_stop_state",
  {
    old_state: systemStopState,
    new_state: systemStopState,
  },
);

export const clearTables = auditLogEventValidator("clear_tables", {});

export const snapshotImport = auditLogEventValidator("snapshot_import", {
  table_names: v.array(
    v.object({
      component: v.union(v.null(), v.string()),
      table_names: v.array(v.string()),
    }),
  ),
  table_count: v.int64(),
  import_mode: snapshotImportMode,
  import_format: snapshotImportFormat,
  requestor: snapshotImportRequestor,
  table_names_deleted: v.array(
    v.object({
      component: v.union(v.null(), v.string()),
      table_names: v.array(v.string()),
    }),
  ),
  table_count_deleted: v.int64(),
});

const componentMetadata = {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
};

const deleteScheduledJobsTable = auditLogEventValidator(
  "delete_scheduled_jobs_table",
  componentMetadata,
);

const deleteTables = auditLogEventValidator("delete_tables", {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  table_names: v.array(v.string()),
});

const deleteComponent = auditLogEventValidator(
  "delete_component",
  componentMetadata,
);

const cancelAllScheduledFunctions = auditLogEventValidator(
  "cancel_all_scheduled_functions",
  componentMetadata,
);

const cancelScheduledFunction = auditLogEventValidator(
  "cancel_scheduled_function",
  {
    component_id: v.union(v.null(), v.string()),
    component: v.union(v.null(), v.string()),
    scheduled_function_id: v.string(),
    function_path: v.union(v.null(), v.string()),
  },
);

const requestExport = auditLogEventValidator("request_export", {
  id: v.string(),
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  format: v.string(),
  requestor: v.string(),
});

const downloadExport = auditLogEventValidator("download_export", {
  id: v.string(),
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  format: v.string(),
  requestor: v.string(),
});

const cancelExport = auditLogEventValidator("cancel_export", {
  id: v.string(),
});

const setExportExpiration = auditLogEventValidator("set_export_expiration", {
  id: v.string(),
  expiration_ts_ms: v.int64(),
});

const auditTopics = v.union(v.array(v.string()), v.null());
const auditOptionalString = v.union(v.string(), v.null());

export const integrationAuditConfig = v.union(
  v.object({ type: v.literal("local"), path: v.string() }),
  v.object({
    type: v.literal("datadog"),
    site: v.string(),
    service: auditOptionalString,
    tags: v.array(v.string()),
    format_version: v.string(),
    topics: auditTopics,
  }),
  v.object({
    type: v.literal("axiom"),
    dataset: v.string(),
    ingest_origin: auditOptionalString,
    attributes: v.array(v.object({ key: v.string(), value: v.string() })),
    format_version: v.string(),
    topics: auditTopics,
  }),
  v.object({
    type: v.literal("webhook"),
    url_origin: auditOptionalString,
    format: v.union(v.literal("json"), v.literal("jsonl")),
    topics: auditTopics,
  }),
  v.object({
    type: v.literal("sentry"),
    dsn_origin: auditOptionalString,
    project_id: v.string(),
    tags: v.union(v.record(v.string(), v.string()), v.null()),
    format_version: v.string(),
  }),
  v.object({
    type: v.literal("postHogLogs"),
    host_origin: auditOptionalString,
    service_name: auditOptionalString,
    topics: auditTopics,
  }),
  v.object({
    type: v.literal("postHogErrorTracking"),
    host_origin: auditOptionalString,
  }),
  v.object({
    type: v.literal("s3Export"),
    bucket: v.string(),
    region: v.string(),
    prefix: auditOptionalString,
    period: syncPeriod,
    selection: syncSelection,
  }),
);

export const integrationAuditCredentialChanges = v.union(
  v.object({ type: v.literal("datadog"), api_key_changed: v.boolean() }),
  v.object({
    type: v.literal("axiom"),
    api_key_changed: v.boolean(),
    ingest_url_changed: v.boolean(),
  }),
  v.object({
    type: v.literal("webhook"),
    hmac_secret_changed: v.boolean(),
    url_changed: v.boolean(),
  }),
  v.object({ type: v.literal("sentry"), dsn_changed: v.boolean() }),
  v.object({
    type: v.literal("postHogLogs"),
    api_key_changed: v.boolean(),
    host_changed: v.boolean(),
  }),
  v.object({
    type: v.literal("postHogErrorTracking"),
    api_key_changed: v.boolean(),
    host_changed: v.boolean(),
  }),
  v.object({
    type: v.literal("s3Export"),
    access_key_id_changed: v.boolean(),
    role_arn_changed: v.optional(v.boolean()),
    secret_access_key_changed: v.boolean(),
  }),
);

const createIntegration = auditLogEventValidator("create_integration", {
  id: v.string(),
  type: v.string(),
  config: v.optional(integrationAuditConfig),
});

const updateIntegration = auditLogEventValidator("update_integration", {
  id: v.string(),
  type: v.string(),
  previous: v.optional(integrationAuditConfig),
  current: v.optional(integrationAuditConfig),
  credential_changes: v.optional(integrationAuditCredentialChanges),
});

const deleteIntegration = auditLogEventValidator("delete_integration", {
  id: v.string(),
  type: v.string(),
  config: v.optional(integrationAuditConfig),
});

const addDocuments = auditLogEventValidator("add_documents", {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  table: v.string(),
  document_ids: v.array(v.string()),
});

const deleteDocuments = auditLogEventValidator("delete_documents", {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  table: v.string(),
  document_ids: v.array(v.string()),
});

const updateDocuments = auditLogEventValidator("update_documents", {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  table: v.string(),
  document_ids: v.array(v.string()),
});

const createTable = auditLogEventValidator("create_table", {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  table: v.string(),
});

const deleteFiles = auditLogEventValidator("delete_files", {
  component_id: v.union(v.null(), v.string()),
  component: v.union(v.null(), v.string()),
  storage_ids: v.array(v.string()),
});

const generateUploadUrl = auditLogEventValidator(
  "generate_upload_url",
  componentMetadata,
);

const createDataSync = auditLogEventValidator("create_data_sync", {
  sync_id: v.string(),
});

const deploymentAuditLogTable = defineTable(
  v.union(
    createEnvironmentVariable,
    deleteEnvironmentVariable,
    updateEnvironmentVariable,
    replaceEnvironmentVariable,
    createUsageLimit,
    updateUsageLimit,
    deleteUsageLimit,
    usageLimitExceeded,
    changeUsageLimitStopState,
    updateCanonicalUrl,
    deleteCanonicalUrl,
    buildIndexes,
    pushConfig,
    pushConfigWithComponents,
    changeDeploymentState,
    pauseDeployment,
    unpauseDeployment,
    changeSystemStopState,
    clearTables,
    snapshotImport,
    deleteScheduledJobsTable,
    deleteTables,
    deleteComponent,
    cancelAllScheduledFunctions,
    cancelScheduledFunction,
    requestExport,
    downloadExport,
    cancelExport,
    setExportExpiration,
    createIntegration,
    updateIntegration,
    deleteIntegration,
    addDocuments,
    deleteDocuments,
    updateDocuments,
    createTable,
    deleteFiles,
    generateUploadUrl,
    createDataSync,
  ),
);

export default deploymentAuditLogTable;
