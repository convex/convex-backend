/* eslint-disable */
/**
 * Generated `api` utility.
 *
 * THIS CODE IS AUTOMATICALLY GENERATED.
 *
 * To regenerate, run `npx convex dev`.
 * @module
 */

import type {
  ApiFromModules,
  FilterApi,
  FunctionReference,
} from "convex/server";
import type * as _system___mocks___server from "../_system/__mocks__/server.js";
import type * as _system_cli_convexUrl from "../_system/cli/convexUrl.js";
import type * as _system_cli_exports from "../_system/cli/exports.js";
import type * as _system_cli_modules from "../_system/cli/modules.js";
import type * as _system_cli_queryEnvironmentVariables from "../_system/cli/queryEnvironmentVariables.js";
import type * as _system_cli_queryImport from "../_system/cli/queryImport.js";
import type * as _system_cli_queryTable from "../_system/cli/queryTable.js";
import type * as _system_cli_tableData from "../_system/cli/tableData.js";
import type * as _system_cli_tableSize from "../_system/cli/tableSize.js";
import type * as _system_cli_tables from "../_system/cli/tables.js";
import type * as _system_frontend_addDocument from "../_system/frontend/addDocument.js";
import type * as _system_frontend_backendState from "../_system/frontend/backendState.js";
import type * as _system_frontend_clearTablePage from "../_system/frontend/clearTablePage.js";
import type * as _system_frontend_common from "../_system/frontend/common.js";
import type * as _system_frontend_components from "../_system/frontend/components.js";
import type * as _system_frontend_convexCloudUrl from "../_system/frontend/convexCloudUrl.js";
import type * as _system_frontend_convexSiteUrl from "../_system/frontend/convexSiteUrl.js";
import type * as _system_frontend_createTable from "../_system/frontend/createTable.js";
import type * as _system_frontend_deleteDocuments from "../_system/frontend/deleteDocuments.js";
import type * as _system_frontend_deploymentEvents from "../_system/frontend/deploymentEvents.js";
import type * as _system_frontend_deploymentState from "../_system/frontend/deploymentState.js";
import type * as _system_frontend_fileStorageV2 from "../_system/frontend/fileStorageV2.js";
import type * as _system_frontend_getById from "../_system/frontend/getById.js";
import type * as _system_frontend_getSchemas from "../_system/frontend/getSchemas.js";
import type * as _system_frontend_getTableMapping from "../_system/frontend/getTableMapping.js";
import type * as _system_frontend_getVersion from "../_system/frontend/getVersion.js";
import type * as _system_frontend_indexes from "../_system/frontend/indexes.js";
import type * as _system_frontend_latestExport from "../_system/frontend/latestExport.js";
import type * as _system_frontend_lib_filters from "../_system/frontend/lib/filters.js";
import type * as _system_frontend_lib_values from "../_system/frontend/lib/values.js";
import type * as _system_frontend_listAuthProviders from "../_system/frontend/listAuthProviders.js";
import type * as _system_frontend_listById from "../_system/frontend/listById.js";
import type * as _system_frontend_listConfiguredSinks from "../_system/frontend/listConfiguredSinks.js";
import type * as _system_frontend_listCronJobRuns from "../_system/frontend/listCronJobRuns.js";
import type * as _system_frontend_listCronJobs from "../_system/frontend/listCronJobs.js";
import type * as _system_frontend_listDeploymentEventsFromTime from "../_system/frontend/listDeploymentEventsFromTime.js";
import type * as _system_frontend_listEnvironmentVariables from "../_system/frontend/listEnvironmentVariables.js";
import type * as _system_frontend_listTableScan from "../_system/frontend/listTableScan.js";
import type * as _system_frontend_modules from "../_system/frontend/modules.js";
import type * as _system_frontend_node from "../_system/frontend/node.js";
import type * as _system_frontend_paginatedDeploymentEvents from "../_system/frontend/paginatedDeploymentEvents.js";
import type * as _system_frontend_paginatedScheduledJobs from "../_system/frontend/paginatedScheduledJobs.js";
import type * as _system_frontend_paginatedTableDocuments from "../_system/frontend/paginatedTableDocuments.js";
import type * as _system_frontend_patchDocumentsFields from "../_system/frontend/patchDocumentsFields.js";
import type * as _system_frontend_replaceDocument from "../_system/frontend/replaceDocument.js";
import type * as _system_frontend_scheduler from "../_system/frontend/scheduler.js";
import type * as _system_frontend_snapshotImport from "../_system/frontend/snapshotImport.js";
import type * as _system_frontend_tableSize from "../_system/frontend/tableSize.js";
import type * as _system_paginationLimits from "../_system/paginationLimits.js";
import type * as _system_repl_wrappers from "../_system/repl/wrappers.js";
import type * as _system_secretSystemTables from "../_system/secretSystemTables.js";
import type * as _system_server from "../_system/server.js";
import type * as tableDefs_awsLambdaVersions from "../tableDefs/awsLambdaVersions.js";
import type * as tableDefs_deploymentAuditLogTable from "../tableDefs/deploymentAuditLogTable.js";
import type * as tableDefs_snapshotImport from "../tableDefs/snapshotImport.js";
import type * as tableDefs_streamingExport from "../tableDefs/streamingExport.js";

/**
 * A utility for referencing Convex functions in your app's API.
 *
 * Usage:
 * ```js
 * const myFunctionReference = api.myModule.myFunction;
 * ```
 */
declare const fullApi: ApiFromModules<{
  "_system/__mocks__/server": typeof _system___mocks___server;
  "_system/cli/convexUrl": typeof _system_cli_convexUrl;
  "_system/cli/exports": typeof _system_cli_exports;
  "_system/cli/modules": typeof _system_cli_modules;
  "_system/cli/queryEnvironmentVariables": typeof _system_cli_queryEnvironmentVariables;
  "_system/cli/queryImport": typeof _system_cli_queryImport;
  "_system/cli/queryTable": typeof _system_cli_queryTable;
  "_system/cli/tableData": typeof _system_cli_tableData;
  "_system/cli/tableSize": typeof _system_cli_tableSize;
  "_system/cli/tables": typeof _system_cli_tables;
  "_system/frontend/addDocument": typeof _system_frontend_addDocument;
  "_system/frontend/backendState": typeof _system_frontend_backendState;
  "_system/frontend/clearTablePage": typeof _system_frontend_clearTablePage;
  "_system/frontend/common": typeof _system_frontend_common;
  "_system/frontend/components": typeof _system_frontend_components;
  "_system/frontend/convexCloudUrl": typeof _system_frontend_convexCloudUrl;
  "_system/frontend/convexSiteUrl": typeof _system_frontend_convexSiteUrl;
  "_system/frontend/createTable": typeof _system_frontend_createTable;
  "_system/frontend/deleteDocuments": typeof _system_frontend_deleteDocuments;
  "_system/frontend/deploymentEvents": typeof _system_frontend_deploymentEvents;
  "_system/frontend/deploymentState": typeof _system_frontend_deploymentState;
  "_system/frontend/fileStorageV2": typeof _system_frontend_fileStorageV2;
  "_system/frontend/getById": typeof _system_frontend_getById;
  "_system/frontend/getSchemas": typeof _system_frontend_getSchemas;
  "_system/frontend/getTableMapping": typeof _system_frontend_getTableMapping;
  "_system/frontend/getVersion": typeof _system_frontend_getVersion;
  "_system/frontend/indexes": typeof _system_frontend_indexes;
  "_system/frontend/latestExport": typeof _system_frontend_latestExport;
  "_system/frontend/lib/filters": typeof _system_frontend_lib_filters;
  "_system/frontend/lib/values": typeof _system_frontend_lib_values;
  "_system/frontend/listAuthProviders": typeof _system_frontend_listAuthProviders;
  "_system/frontend/listById": typeof _system_frontend_listById;
  "_system/frontend/listConfiguredSinks": typeof _system_frontend_listConfiguredSinks;
  "_system/frontend/listCronJobRuns": typeof _system_frontend_listCronJobRuns;
  "_system/frontend/listCronJobs": typeof _system_frontend_listCronJobs;
  "_system/frontend/listDeploymentEventsFromTime": typeof _system_frontend_listDeploymentEventsFromTime;
  "_system/frontend/listEnvironmentVariables": typeof _system_frontend_listEnvironmentVariables;
  "_system/frontend/listTableScan": typeof _system_frontend_listTableScan;
  "_system/frontend/modules": typeof _system_frontend_modules;
  "_system/frontend/node": typeof _system_frontend_node;
  "_system/frontend/paginatedDeploymentEvents": typeof _system_frontend_paginatedDeploymentEvents;
  "_system/frontend/paginatedScheduledJobs": typeof _system_frontend_paginatedScheduledJobs;
  "_system/frontend/paginatedTableDocuments": typeof _system_frontend_paginatedTableDocuments;
  "_system/frontend/patchDocumentsFields": typeof _system_frontend_patchDocumentsFields;
  "_system/frontend/replaceDocument": typeof _system_frontend_replaceDocument;
  "_system/frontend/scheduler": typeof _system_frontend_scheduler;
  "_system/frontend/snapshotImport": typeof _system_frontend_snapshotImport;
  "_system/frontend/tableSize": typeof _system_frontend_tableSize;
  "_system/paginationLimits": typeof _system_paginationLimits;
  "_system/repl/wrappers": typeof _system_repl_wrappers;
  "_system/secretSystemTables": typeof _system_secretSystemTables;
  "_system/server": typeof _system_server;
  "tableDefs/awsLambdaVersions": typeof tableDefs_awsLambdaVersions;
  "tableDefs/deploymentAuditLogTable": typeof tableDefs_deploymentAuditLogTable;
  "tableDefs/snapshotImport": typeof tableDefs_snapshotImport;
  "tableDefs/streamingExport": typeof tableDefs_streamingExport;
}>;
export declare const api: FilterApi<
  typeof fullApi,
  FunctionReference<any, "public">
>;
export declare const internal: FilterApi<
  typeof fullApi,
  FunctionReference<any, "internal">
>;
