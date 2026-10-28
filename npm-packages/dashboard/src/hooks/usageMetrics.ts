import { DatabricksQueryId, DateRange, useUsageQuery } from "api/usage";
import { DeploymentType } from "generatedApi";

const QUERY_IDS_: {
  summary: DatabricksQueryId;
  backupStorageEstimate: DatabricksQueryId;
  functionBreakdown: DatabricksQueryId;
  deploymentsByClassAndRegion: DatabricksQueryId;
  deploymentCountByType: DatabricksQueryId;
  deploymentCountByStatus: DatabricksQueryId;
} = {
  summary: "b63fe48d-320c-401a-8682-0a0b36b50e2b",
  backupStorageEstimate: "b58b3969-d916-460a-a7b6-210af082b24a",
  functionBreakdown: "76c86baa-418e-4d7f-ac21-46f397030595",
  deploymentsByClassAndRegion: "dfc73057-1948-4b99-a3bf-9ae802a395ee",
  deploymentCountByType: "34801c2e-06a8-4cc5-8ecc-dd412b908763",
  deploymentCountByStatus: "4bc3e942-951a-440a-be2a-7c833b77eee1",
};

const BY_PROJECT_QUERY_IDS_: {
  databaseStorageByProjectAndClass: DatabricksQueryId;
  databaseStorageByTable: DatabricksQueryId;
  documentCountByTable: DatabricksQueryId;
  documentCountByProject: DatabricksQueryId;
  databaseIOByProjectAndClass: DatabricksQueryId;
  functionCallsByProjectAndClass: DatabricksQueryId;
  storageCallsByProjectAndClass: DatabricksQueryId;
  computeByProject: DatabricksQueryId;
  computeByProjectSelfServe: DatabricksQueryId;
  fileStorageByProject: DatabricksQueryId;
  searchStorageByProject: DatabricksQueryId;
  dataEgressByProject: DatabricksQueryId;
  auditLogBandwidthByProject: DatabricksQueryId;
  searchQueriesByProject: DatabricksQueryId;
  deploymentCountByProject: DatabricksQueryId;
  aiGatewayCostByProject: DatabricksQueryId;
  aiGatewayCostByModel: DatabricksQueryId;
} = {
  databaseStorageByProjectAndClass: "489b0f87-6b3a-4dfe-a327-f2965b5c2977",
  databaseStorageByTable: "017c5977-3002-40ca-96af-31868e70e611",
  documentCountByTable: "28646a64-f234-44c2-b763-ecb63d43ad24",
  documentCountByProject: "2a5120a1-b334-4d99-b378-1028487c2202",
  databaseIOByProjectAndClass: "9f606f77-521d-44bb-83ef-b1057b0fb1c9",
  functionCallsByProjectAndClass: "77a4e5bd-aa82-43e7-85a4-89897cecaa05",
  storageCallsByProjectAndClass: "90c9d3b3-d93e-4583-a054-dbb2f9dad5a3",
  computeByProject: "45921934-b4a2-4c91-9ba2-8987fba6e8e3",
  computeByProjectSelfServe: "038e5492-6de5-4ddb-86b4-761e19b4d2ab",
  fileStorageByProject: "72add9df-4ef2-47fe-9942-194dfbb72088",
  searchStorageByProject: "87f2b0b2-024c-4c2a-bf81-8a3c0cab1b82",
  dataEgressByProject: "67ce838f-b2d0-4cda-9a2e-580c6d134466",
  auditLogBandwidthByProject: "158b5d9c-a1d5-469d-a7ca-b610cb59c978",
  searchQueriesByProject: "48ae8bb1-ec17-41db-9e35-7c774296c5ac",
  deploymentCountByProject: "0b6c9ab3-c17c-4ad5-bfca-8f0300e494f6",
  aiGatewayCostByProject: "e0b23b66-f3de-42ba-a2e0-547c927519d5",
  aiGatewayCostByModel: "b09fd03c-c13c-42f1-821b-d315e1238045",
};

// --- Types ---

export type UsageSummaryRow = {
  deploymentClass: string;
  region: string;
  databaseStorage: number;
  databaseIO: number;
  functionCalls: number;
  queryMutationCompute: number; // GB-hours
  actionComputeConvex: number; // GB-hours
  actionComputeNode: number; // GB-hours
  fileStorage: number;
  searchStorage: number;
  dataEgress: number;
  searchQueries: number;
  actionComputeUser: number; // GB-hours — corrected non-node compute for business plans
  auditLogBandwidth: number;
  // Current deployment count gauge (team-wide; not filtered by project/component).
  deploymentCount: number;
  pausedDeploymentCount: number;
  idleDeploymentCount: number;
};

export type BackupStorageSummary = {
  databaseStorage: number;
  fileStorage: number;
};

// Function name of the row the breakdown query emits per project to carry the
// functions that fall outside the top-N it returns, so project totals stay exact.
export const REST_OF_FUNCTIONS = "_rest";

export type UsageDeploymentType = DeploymentType | "local";

export interface AggregatedFunctionMetrics {
  function: string;
  projectId: number;
  callCount: number;
  databaseIngressSize: number;
  databaseEgressSize: number;
  textSearchGb: number; // query-GB
  vectorSearchGb: number; // query-GB
  queryMutationComputeTime: number; // GB-hours
  actionComputeConvexTime: number; // GB-hours
  actionComputeNodeTime: number; // GB-hours
  dataEgress: number; // bytes
  aiGatewayCost: number; // dollars
  // Null on the `_rest` row, which spans every deployment type.
  deploymentType: UsageDeploymentType | null;
  componentPath: string;
}

export interface DailyPerTagMetricsByProjectAndClass {
  ds: string;
  projectId: number | "_rest";
  deploymentClass: string;
  metrics: { tag: string; value: number }[];
}

export type DailyMetric = {
  ds: string;
  value: number;
};

export interface DailyPerTagMetrics {
  ds: string;
  metrics: { tag: string; value: number }[];
}

export interface DailyMetricByTable {
  ds: string;
  projectId: number | "_rest";
  tableName: string;
  value: number;
}

export interface DailyMetricByProject {
  ds: string;
  projectId: number | "_rest";
  value: number;
}

export interface DailyPerTagMetricsByProject {
  ds: string;
  projectId: number | "_rest";
  metrics: { tag: string; value: number }[];
}

export interface DailyDeploymentsByClassAndRegion {
  ds: string;
  deploymentClass: string;
  region: string;
  count: number;
}

// --- Helpers ---

function parseProjectId(projectId: string | number): number | "_rest" {
  if (projectId === "_rest") {
    return "_rest";
  }
  return Number(projectId);
}

// --- Hooks ---

export function useUsageTeamSummary(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
  options: { deploymentName?: string; skip?: boolean } = {},
) {
  const { data, error } = useUsageQuery({
    queryId: QUERY_IDS_.summary,
    teamId,
    projectId,
    period,
    componentPrefix,
    deploymentName: options.deploymentName,
    skip: options.skip,
  });

  if (error) {
    return { data: undefined, error };
  }

  if (!data) {
    return { data: undefined, error: undefined };
  }

  return {
    data: data.map(
      ([
        _teamId,
        deploymentClass,
        region,
        databaseStorage,
        databaseIO,
        functionCalls,
        queryMutationCompute,
        actionComputeConvex,
        actionComputeNode,
        fileStorage,
        searchStorage,
        dataEgress,
        searchQueries,
        actionComputeUser,
        deploymentCount,
        pausedDeploymentCount,
        idleDeploymentCount,
        auditLogBandwidth,
      ]) =>
        ({
          deploymentClass,
          region,
          databaseStorage: Number(databaseStorage),
          databaseIO: Number(databaseIO),
          functionCalls: Number(functionCalls),
          queryMutationCompute: Number(queryMutationCompute) / 60 / 60,
          actionComputeConvex: Number(actionComputeConvex) / 60 / 60,
          actionComputeNode: Number(actionComputeNode) / 60 / 60,
          fileStorage: Number(fileStorage),
          searchStorage: Number(searchStorage),
          dataEgress: Number(dataEgress),
          searchQueries: Number(searchQueries),
          actionComputeUser: Number(actionComputeUser) / 60 / 60,
          deploymentCount: Number(deploymentCount),
          pausedDeploymentCount: Number(pausedDeploymentCount),
          idleDeploymentCount: Number(idleDeploymentCount),
          auditLogBandwidth: Number(auditLogBandwidth),
        }) satisfies UsageSummaryRow,
    ),
    error: undefined,
  };
}

export function useBackupStorageSummary(
  teamId: number,
  projectId: number,
  deploymentName: string | undefined,
  skip = false,
) {
  const { data, error } = useUsageQuery({
    queryId: QUERY_IDS_.backupStorageEstimate,
    teamId,
    projectId,
    deploymentName,
    period: null,
    componentPrefix: null,
    skip,
  });

  if (error) {
    return { data: undefined, error };
  }

  const row = data?.[0];
  return {
    data: row
      ? {
          databaseStorage: Number(row[0]),
          fileStorage: Number(row[1]),
        }
      : undefined,
    error: undefined,
  };
}

export function useUsageTeamMetricsByFunction(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: AggregatedFunctionMetrics[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: QUERY_IDS_.functionBreakdown,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([
        _teamId,
        functionName,
        projectIdField,
        callCount,
        databaseIngressSize,
        databaseEgressSize,
        textSearchGb,
        vectorSearchGb,
        queryMutationComputeTime,
        actionComputeConvexTime,
        actionComputeNodeTime,
        dataEgress,
        deploymentType,
        componentPath,
        aiGatewayCost,
      ]) => ({
        function: functionName,
        projectId: Number(projectIdField),
        callCount: Number(callCount),
        databaseIngressSize: Number(databaseIngressSize),
        databaseEgressSize: Number(databaseEgressSize),
        textSearchGb: Number(textSearchGb),
        vectorSearchGb: Number(vectorSearchGb),
        queryMutationComputeTime: Number(queryMutationComputeTime) / 60 / 60,
        actionComputeConvexTime: Number(actionComputeConvexTime) / 60 / 60,
        actionComputeNodeTime: Number(actionComputeNodeTime) / 60 / 60,
        dataEgress: Number(dataEgress),
        // SQL NULL arrives as "" (see useUsageQuery).
        deploymentType: (deploymentType || null) as UsageDeploymentType | null,
        componentPath,
        aiGatewayCost: Number(aiGatewayCost),
      }),
    ),
    error: undefined,
  };
}

// Daily by-project hooks (with deployment class)

export function useDatabaseStoragePerDayByProjectAndClass(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProjectAndClass[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.databaseStorageByProjectAndClass,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([
        _teamId,
        projectId,
        deploymentClass,
        ds,
        documentStorage,
        indexStorage,
      ]) => ({
        ds,
        projectId: parseProjectId(projectId),
        deploymentClass,
        metrics: [
          { tag: "document", value: Number(documentStorage) },
          { tag: "index", value: Number(indexStorage) },
        ],
      }),
    ),
    error: undefined,
  };
}

export function useDatabaseStoragePerDayByTable(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyMetricByTable[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.databaseStorageByTable,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([
        _teamId,
        rowProjectId,
        tableName,
        ds,
        documentStorage,
        indexStorage,
      ]) => ({
        ds,
        projectId: parseProjectId(rowProjectId),
        tableName,
        value: Number(documentStorage) + Number(indexStorage),
      }),
    ),
    error: undefined,
  };
}

export function useDocumentCountPerDayByTable(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyMetricByTable[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.documentCountByTable,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([_teamId, rowProjectId, tableName, ds, documentCount]) => ({
        ds,
        projectId: parseProjectId(rowProjectId),
        tableName,
        value: Number(documentCount),
      }),
    ),
    error: undefined,
  };
}

export function useDatabaseIOPerDayByProjectAndClass(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProjectAndClass[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.databaseIOByProjectAndClass,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([_teamId, projectId, deploymentClass, ds, ingressSize, egressSize]) => ({
        ds,
        projectId: parseProjectId(projectId),
        deploymentClass,
        metrics: [
          { tag: "egress", value: Number(egressSize) },
          { tag: "ingress", value: Number(ingressSize) },
        ],
      }),
    ),
    error: undefined,
  };
}

export function useComputePerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.computeByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([
        _teamId,
        projectId,
        ds,
        queryMutationGbS,
        actionConvexGbS,
        actionNodeGbS,
      ]) => ({
        ds,
        projectId: parseProjectId(projectId),
        metrics: [
          {
            tag: "queryMutation",
            value: Number(queryMutationGbS) / 60 / 60,
          },
          {
            tag: "actionConvex",
            value: Number(actionConvexGbS) / 60 / 60,
          },
          { tag: "actionNode", value: Number(actionNodeGbS) / 60 / 60 },
        ],
      }),
    ),
    error: undefined,
  };
}

export function useComputePerDayByProjectSelfServe(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.computeByProjectSelfServe,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([_teamId, projectId, ds, actionConvexGbS, actionNodeGbS]) => ({
        ds,
        projectId: parseProjectId(projectId),
        metrics: [
          {
            tag: "actionConvex",
            value: Number(actionConvexGbS) / 60 / 60,
          },
          { tag: "actionNode", value: Number(actionNodeGbS) / 60 / 60 },
        ],
      }),
    ),
    error: undefined,
  };
}

export function useFunctionCallsPerDayByProjectAndClass(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProjectAndClass[] | undefined; error: any } {
  const { data: functionData, error: functionError } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.functionCallsByProjectAndClass,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  const { data: storageData, error: storageError } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.storageCallsByProjectAndClass,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (functionError || storageError) {
    return { data: undefined, error: functionError || storageError };
  }

  const metrics = functionData?.map(
    ([
      _teamId,
      projectId,
      deploymentClass,
      ds,
      cachedQueries,
      uncachedQueries,
      mutations,
      actions,
      httpActions,
    ]) => ({
      ds,
      projectId: parseProjectId(projectId),
      deploymentClass,
      metrics: [
        { tag: "uncached_query", value: Number(uncachedQueries) },
        { tag: "cached_query", value: Number(cachedQueries) },
        { tag: "mutation", value: Number(mutations) },
        { tag: "action", value: Number(actions) },
        { tag: "http_action", value: Number(httpActions) },
      ],
    }),
  );

  // Augment with storage calls data
  const storageDataByKey = (storageData || []).reduce(
    (acc, [_teamId, projectId, deploymentClass, ds, storageCalls]) => {
      const key = `${ds}-${projectId}-${deploymentClass}`;
      acc[key] = Number(storageCalls);
      return acc;
    },
    {} as Record<string, number>,
  );
  for (const metric of metrics || []) {
    const key = `${metric.ds}-${metric.projectId}-${metric.deploymentClass}`;
    const storageCalls = storageDataByKey[key];
    if (storageCalls) {
      metric.metrics.push({ tag: "storage_api", value: storageCalls });
      delete storageDataByKey[key];
    }
  }

  return { data: metrics, error: undefined };
}

// Daily by-project hooks (without deployment class)

export function useFileStoragePerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyMetricByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.fileStorageByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, projectId, ds, fileStorage]) => ({
      ds,
      projectId: parseProjectId(projectId),
      value: Number(fileStorage),
    })),
    error: undefined,
  };
}

export function useSearchStoragePerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.searchStorageByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([_teamId, projectId, ds, textSearchStorage, vectorStorage]) => ({
        ds,
        projectId: parseProjectId(projectId),
        metrics: [
          { tag: "textSearch", value: Number(textSearchStorage) },
          { tag: "vector", value: Number(vectorStorage) },
        ],
      }),
    ),
    error: undefined,
  };
}

export function useDataEgressPerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.dataEgressByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([
        _teamId,
        projectId,
        ds,
        servingEgress,
        userFunctionEgress,
        cloudRestore,
        cloudBackup,
        snapshotExport,
        snapshotImport,
        fetchEgress,
        logStreamEgress,
        streamingExportEgress,
      ]) => ({
        ds,
        projectId: parseProjectId(projectId),
        metrics: [
          { tag: "fetchEgress", value: Number(fetchEgress) },
          { tag: "logStream", value: Number(logStreamEgress) },
          { tag: "streamingExport", value: Number(streamingExportEgress) },
          { tag: "servingEgress", value: Number(servingEgress) },
          { tag: "userFunctionEgress", value: Number(userFunctionEgress) },
          {
            tag: "backup",
            value: Number(cloudBackup) + Number(snapshotExport),
          },
          {
            tag: "restore",
            value: Number(cloudRestore) + Number(snapshotImport),
          },
        ],
      }),
    ),
    error: undefined,
  };
}

export function useAuditLogBandwidthPerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyMetricByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.auditLogBandwidthByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, projectId, ds, auditLogEgress]) => ({
      ds,
      projectId: parseProjectId(projectId),
      value: Number(auditLogEgress),
    })),
    error: undefined,
  };
}

// Measured in dollars
export function useAiGatewayCostPerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyMetricByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.aiGatewayCostByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, projectId, ds, aiGatewayCost]) => ({
      ds,
      projectId: parseProjectId(projectId),
      value: Number(aiGatewayCost),
    })),
    error: undefined,
  };
}

// Measured in dollars
export function useAiGatewayCostPerDayByModel(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetrics[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.aiGatewayCostByModel,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  if (data === undefined) {
    return { data: undefined, error: undefined };
  }

  const byDay = new Map<string, { tag: string; value: number }[]>();
  for (const [_teamId, model, ds, aiGatewayCost] of data) {
    if (ds === null || ds === undefined) {
      continue;
    }
    const metrics = byDay.get(ds) ?? [];
    // SQL NULL arrives as "" (see useUsageQuery).
    metrics.push({ tag: model || "unknown", value: Number(aiGatewayCost) });
    byDay.set(ds, metrics);
  }
  return {
    data: [...byDay.entries()].map(([ds, metrics]) => ({ ds, metrics })),
    error: undefined,
  };
}

export function useSearchQueriesPerDayByProject(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetricsByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.searchQueriesByProject,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(
      ([_teamId, projectId, ds, textSearchGb, vectorSearchGb]) => ({
        ds,
        projectId: parseProjectId(projectId),
        metrics: [
          { tag: "textSearch", value: Number(textSearchGb) },
          { tag: "vectorSearch", value: Number(vectorSearchGb) },
        ],
      }),
    ),
    error: undefined,
  };
}

// Deployments

export function useDeploymentsByClassAndRegion(
  teamId: number,
  period: DateRange | null,
): { data: DailyDeploymentsByClassAndRegion[] | undefined; error: any } {
  // This query is not broken down by project, so it is always team-wide.
  const { data, error } = useUsageQuery({
    queryId: QUERY_IDS_.deploymentsByClassAndRegion,
    teamId,
    projectId: null,
    period,
    componentPrefix: null,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, ds, deploymentClass, region, count]) => ({
      ds,
      deploymentClass,
      region,
      count: Number(count),
    })),
    error: undefined,
  };
}

export function useUsageTeamDocumentsPerDayByProject(
  teamId: number,
  period: DateRange | null,
  componentPrefix: string | null,
): { data: DailyMetricByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.documentCountByProject,
    teamId,
    projectId: null,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, projectId, ds, count]) => ({
      ds,
      projectId: parseProjectId(projectId),
      value: Number(count),
    })),
    error: undefined,
  };
}

export function useUsageTeamDeploymentCountPerDayByProject(
  teamId: number,
  period: DateRange | null,
  componentPrefix: string | null,
): { data: DailyMetricByProject[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: BY_PROJECT_QUERY_IDS_.deploymentCountByProject,
    teamId,
    projectId: null,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, projectId, ds, count]) => ({
      ds,
      projectId: parseProjectId(projectId),
      value: Number(count),
    })),
    error: undefined,
  };
}

export function useUsageTeamDeploymentCountByType(
  teamId: number,
  period: DateRange | null,
  projectId: number | null,
  componentPrefix: string | null,
): { data: DailyPerTagMetrics[] | undefined; error: any } {
  const { data, error } = useUsageQuery({
    queryId: QUERY_IDS_.deploymentCountByType,
    teamId,
    projectId,
    period,
    componentPrefix,
  });

  if (error) {
    return { data: undefined, error };
  }

  if (data === undefined) {
    return { data: undefined, error: undefined };
  }

  // Group by date since each row is [teamId, deploymentType, ds, count]
  const groupedByDate = new Map<string, Map<string, number>>();

  data.forEach(([_teamId, deploymentType, ds, count]) => {
    if (!groupedByDate.has(ds)) {
      groupedByDate.set(ds, new Map());
    }
    const tag = deploymentType || "deleted";
    groupedByDate.get(ds)!.set(tag, Number(count));
  });

  return {
    data: Array.from(groupedByDate.entries()).map(([ds, metricsMap]) => ({
      ds,
      metrics: Array.from(metricsMap.entries()).map(([tag, value]) => ({
        tag,
        value,
      })),
    })),
    error: undefined,
  };
}

// Active vs. idle vs. paused deployment counts per day. The three are mutually
// exclusive: "paused" and "idle" come from the daily gauge, and "active" is the
// remainder (total - paused - idle).
export function useUsageTeamDeploymentCountByStatus(
  teamId: number,
  period: DateRange | null,
): { data: DailyPerTagMetrics[] | undefined; error: any } {
  // This query reads the team-wide deployment count gauge, so it is not broken
  // down by project or component.
  const { data, error } = useUsageQuery({
    queryId: QUERY_IDS_.deploymentCountByStatus,
    teamId,
    projectId: null,
    period,
    componentPrefix: null,
  });

  if (error) {
    return { data: undefined, error };
  }

  return {
    data: data?.map(([_teamId, ds, active, paused, idle]) => ({
      ds,
      metrics: [
        { tag: "active", value: Number(active) },
        { tag: "idle", value: Number(idle) },
        { tag: "paused", value: Number(paused) },
      ],
    })),
    error: undefined,
  };
}
