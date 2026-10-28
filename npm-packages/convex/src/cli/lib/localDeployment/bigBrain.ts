import { Context } from "../../../bundler/context.js";
import { bigBrainAPI } from "../utils/utils.js";

export async function bigBrainStart(
  ctx: Context,
  data: {
    // cloud port
    port: number;
    projectSlug: string;
    teamSlug: string;
    instanceName: string | null;
  },
): Promise<{
  deploymentName: string;
  /** @deprecated */
  adminKey: string;
  projectId: number;
}> {
  return bigBrainAPI({
    ctx,
    method: "POST",
    path: "local_deployment/start",
    data,
  });
}

export async function bigBrainPause(
  ctx: Context,
  data: {
    projectSlug: string;
    teamSlug: string;
  },
): Promise<void> {
  return bigBrainAPI({
    ctx,
    method: "POST",
    path: "local_deployment/pause",
    data,
  });
}

export async function bigBrainRecordActivity(
  ctx: Context,
  data: {
    instanceName: string;
    adminKey: string;
  },
) {
  return bigBrainAPI({
    ctx,
    method: "POST",
    path: "local_deployment/record_activity",
    data,
  });
}

export async function bigBrainEnableFeatureMetadata(
  ctx: Context,
): Promise<{ totalProjects: { kind: "none" | "one" | "multiple" } }> {
  return bigBrainAPI({
    ctx,
    method: "POST",
    path: "local_deployment/enable_feature_metadata",
    data: {},
  });
}

/** Whether a project already has a cloud dev deployment for this user. */
export async function projectHasExistingCloudDev(
  ctx: Context,
  {
    projectSlug,
    teamSlug,
  }: {
    projectSlug: string;
    teamSlug: string;
  },
) {
  const response = await bigBrainAPI<
    | {
        kind: "Exists";
      }
    | {
        kind: "DoesNotExist";
      }
  >({
    ctx,
    method: "POST",
    path: "deployment/existing_dev",
    data: { projectSlug, teamSlug },
  });
  if (response.kind === "Exists") {
    return true;
  } else if (response.kind === "DoesNotExist") {
    return false;
  }
  return await ctx.crash({
    exitCode: 1,
    errorType: "fatal",
    printedMessage: `Unexpected /api/deployment/existing_dev response: ${JSON.stringify(response, null, 2)}`,
  });
}
