import { Infer } from "convex/values";
import {
  axiomConfig,
  datadogConfig,
  postHogErrorTrackingConfig,
  postHogLogsConfig,
  s3ExportConfig,
  udfType,
  udfVisibility,
  webhookConfig,
} from "../../schema";
import { Doc } from "../../_generated/dataModel";

export type UdfType = Infer<typeof udfType>;
export type Visibility = Infer<typeof udfVisibility>;

export type UdfWrite = {
  path: string;
  source: string;
};

export type LogLevel = "LOG" | "DEBUG" | "INFO" | "WARN" | "ERROR";

export type StructuredLogLine = {
  messages: string[];
  level: LogLevel;
  timestamp: number;
  isTruncated: boolean;
  componentPath?: string;
  udfPath?: string;
};
export type LogLine = string | StructuredLogLine;

export type UsageStats = {
  memoryUsedMb: number | null;
  databaseReadBytes: number;
  databaseWriteBytes: number;
  databaseIoReadBytes: number;
  databaseIoWriteBytes: number;
  databaseReadDocuments: number;
  storageReadBytes: number;
  storageWriteBytes: number;
  vectorIndexReadBytes: number;
  vectorIndexWriteBytes: number;
  textIndexQueryBytes: number;
  textIndexWriteQueryBytes: number;
  vectorIndexReadQueryBytes: number;
  vectorIndexWriteQueryBytes: number;
  networkEgressBytes: number;
};

export type FunctionExecutionCompletion = {
  kind: "Completion";
  componentPath?: string;
  identifier: string;
  udfType: UdfType;
  arguments: string[];
  logLines: LogLine[];
  // Unix timestamp (in seconds)
  timestamp: number;

  // null, except for successful http udfs where the status code will be
  // present. Always null for function udfs.
  // For http udfs, status is a string, but always a numeric value (200, 500
  // etc).
  success: { status: string } | null;
  error: string | null;

  cachedResult: boolean;
  // UDF execution duration (in seconds)
  executionTime: number;

  requestId: string;
  executionId: string;
  usageStats?: UsageStats;
  returnBytes?: number;
  caller: string;
  environment: string;
  identityType: string;
  parentExecutionId: string | null;
  executionTimestamp?: number;
};

export type FunctionExecutionProgess = {
  kind: "Progress";
  componentPath?: string | null;
  identifier: string;
  udfType: UdfType;
  // Unix timestamp (in seconds)
  timestamp: number;
  logLines: LogLine[];
  requestId: string;
  executionId: string;
};

export type FunctionExecution =
  | FunctionExecutionCompletion
  | FunctionExecutionProgess;

export type ResolvedSourcePos = {
  path: string;
  start_lineno: bigint;
  start_col: bigint;
};

export type AnalyzedModuleFunction = {
  name: string;
  lineno?: number;
  udfType: UdfType;
  visibility: Visibility;
};

// To deprecate
export type Module = {
  functions: AnalyzedModuleFunction[];
  cronSpecs?: [string, CronSpec][];
  // By returning sourcePackageId, we make module queries rerender when source
  // code changes, which allows the dashboard to refetch source code.
  sourcePackageId: string;
};

export type CronJob = Doc<"_cron_jobs">;
export type CronSpec = Doc<"_cron_jobs">["cronSpec"];
export type CronSchedule = Doc<"_cron_jobs">["cronSpec"]["cronSchedule"];
export type CronJobLog = Doc<"_cron_job_logs">;
export type CronJobWithRuns = Doc<"_cron_jobs"> & {
  lastRun: CronJobLog | null;
  nextRun: Doc<"_cron_next_run">;
};

export type Modules = Map<string, Module>;

export type Export = Doc<"_exports">;

export type CompletedExport = Export & { state: "completed" };

export type EnvironmentVariable = Doc<"_environment_variables">;

export type ScheduledJob = Doc<"_scheduled_jobs">;

export type UdfConfig = Doc<"_udf_config">;

// The `_log_sinks` document as it is stored on the deployment, credentials
// included.
type StoredIntegration = Doc<"_log_sinks">;

// A configured integration as a client sees it: `listConfiguredSinks` strips
// the fields below, which are accepted on create/update but never returned.
export type Integration = Omit<StoredIntegration, "config"> & {
  config: IntegrationConfig;
  exportProgress?: Doc<"_s3_export_state">["checkpoint"]["progress"];
};

// The AWS secret access key stays on the deployment. Callers omit it on update
// to keep the stored value.
export type RedactedS3ExportConfig = Omit<S3ExportConfig, "secretAccessKey">;

// Note: doesn't include exports or auth
export type IntegrationConfig =
  | Exclude<StoredIntegration["config"], { type: "s3Export" }>
  | RedactedS3ExportConfig;

// Streaming export/import integrations aren't configured on convex's side, so
// they don't use the _log_sinks table.
export type ExportIntegrationType = "fivetran";
export type ImportIntegrationType = "airbyte";
// WorkOS environment mappings live in api.convex.dev for now
export type AuthIntegrationType = "workos";
export type IntegrationType =
  | IntegrationConfig["type"]
  | ExportIntegrationType
  | ImportIntegrationType
  | AuthIntegrationType;

export type DatadogConfig = Infer<typeof datadogConfig>;

export type DatadogSiteLocation = DatadogConfig["siteLocation"];

export type WebhookConfig = Infer<typeof webhookConfig>;

export type AxiomConfig = Infer<typeof axiomConfig>;

export type PostHogLogsConfig = Infer<typeof postHogLogsConfig>;

export type PostHogErrorTrackingConfig = Infer<
  typeof postHogErrorTrackingConfig
>;

export type S3ExportConfig = Infer<typeof s3ExportConfig>;

export type SyncPeriod = S3ExportConfig["period"];

export type SyncSelection = S3ExportConfig["selection"];
