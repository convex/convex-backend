import { ProjectResponse } from "@convex-dev/platform/managementApi";
import { Context } from "../../../bundler/context.js";
import { logVerbose } from "../../../bundler/log.js";
import {
  DeploymentSelection,
  getProjectDetails,
} from "../deploymentSelection.js";
import { ParsedDeploymentSelector } from "../deploymentSelector.js";
import { typedPlatformClient } from "../utils/utils.js";
import { bigBrainPause } from "./bigBrain.js";
import { LocalDeploymentConfig } from "./filePaths.js";

/**
 * Returns the cloud project the user is targeting with a `[team:project:]local`
 * selector, or `null` if the project context can't be determined (e.g.
 * anonymous mode, no current project).
 */
export async function targetProjectForLocalSelector(
  ctx: Context,
  parsed: ParsedDeploymentSelector,
  currentSelection: DeploymentSelection,
): Promise<ProjectResponse | null> {
  switch (parsed.kind) {
    case "inTeamProject":
      return await getProjectDetails(ctx, {
        kind: "teamAndProjectSlugs",
        teamSlug: parsed.teamSlug,
        projectSlug: parsed.projectSlug,
      });
    case "inCurrentProject":
      if (currentSelection.kind !== "deploymentWithinProject") return null;
      return await getProjectDetails(ctx, currentSelection.targetProject);
    case "inProject": {
      if (currentSelection.kind !== "deploymentWithinProject") return null;
      // For `project:local` we keep the team from the current cloud project
      // and switch only the project slug.
      const current = await getProjectDetails(
        ctx,
        currentSelection.targetProject,
      );
      return await getProjectDetails(ctx, {
        kind: "teamAndProjectSlugs",
        teamSlug: current.teamSlug,
        projectSlug: parsed.projectSlug,
      });
    }
    default:
      return null;
  }
}

/**
 * Compares the cloud project association stored in `config.json` against the
 * cloud project the user is asking about. Returns `"skip"` when the on-disk
 * config has no `cloudProjectId` (older configs, anonymous mode).
 */
export function checkLocalConfigMatchesProject(
  _ctx: Context,
  localConfig: LocalDeploymentConfig,
  target: { id: number },
): "match" | "mismatch" | "skip" {
  if (localConfig.cloudProjectId === undefined) {
    return "skip";
  }
  return localConfig.cloudProjectId === target.id ? "match" : "mismatch";
}

/**
 * Best-effort lookup of a cloud project's team/project slugs by id. Returns
 * `null` if the project can't be resolved (e.g. it was deleted from the
 * dashboard).
 */
export async function getCloudProjectSlugsBestEffort(
  ctx: Context,
  cloudProjectId: number,
): Promise<{ teamSlug: string; slug: string } | null> {
  try {
    const result = await typedPlatformClient(ctx, { throw: true }).GET(
      "/projects/{project_id}",
      {
        params: { path: { project_id: cloudProjectId } },
      },
    );
    const project = result.data;
    if (!project) return null;
    return { teamSlug: project.teamSlug, slug: project.slug };
  } catch (e) {
    logVerbose(
      `Failed to resolve cloud project ${cloudProjectId}: ${e as any}`,
    );
    return null;
  }
}

/**
 * Best-effort call to `local_deployment/pause` for the local deployment
 * associated with the given cloud project. Swallows any errors.
 */
export async function pauseLocalDeploymentBestEffort(
  ctx: Context,
  project: { teamSlug: string; slug: string } | null,
): Promise<void> {
  if (project === null) return;
  try {
    await bigBrainPause(ctx, {
      teamSlug: project.teamSlug,
      projectSlug: project.slug,
    });
  } catch (e) {
    logVerbose(`Failed to pause local deployment: ${e as any}`);
  }
}
