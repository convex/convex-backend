import { Command } from "@commander-js/extra-typings";
import { Context, oneoffContext } from "../bundler/context.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { logFinishedStep } from "../bundler/log.js";
import { announceDeploymentTarget } from "./lib/announceDeploymentTarget.js";
import {
  DeploymentSelection,
  getDeploymentSelection,
  deploymentNameFromSelection,
} from "./lib/deploymentSelection.js";
import {
  parseDeploymentSelector,
  ParsedDeploymentSelector,
} from "./lib/deploymentSelector.js";
import {
  deploymentUrlForApplicationEnv,
  updateEnvAndConfigForDeploymentSelection,
} from "./configure.js";
import { fetchDeploymentCanonicalUrls } from "./lib/deploy2.js";
import {
  loadProjectLocalConfig,
  saveDeploymentConfig,
} from "./lib/localDeployment/filePaths.js";
import {
  checkLocalConfigMatchesProject,
  getCloudProjectSlugsBestEffort,
  pauseLocalDeploymentBestEffort,
  targetProjectForLocalSelector,
} from "./lib/localDeployment/projectMismatch.js";
import { bigBrainStart } from "./lib/localDeployment/bigBrain.js";
import { promptYesNo } from "./lib/utils/prompts.js";
import { createLocalDeployment } from "./deploymentCreate.js";
import { chalkStderr } from "chalk";
import { logWarning } from "../bundler/log.js";

export const deploymentSelect = new Command("select")
  .summary("Select the deployment to use when running commands")
  .description(
    [
      "Select the deployment to use when running commands.",
      "",
      "The deployment will be used by all `npx convex` commands, except `npx convex deploy`. You can also run individual commands on another deployment by using the --deployment flag on that command.",
      "",
      "• Select your personal cloud dev deployment in the current project: `npx convex deployment select dev`",
      "• Select your local deployment: `npx convex deployment select local`",
      "• Select a deployment in the same project by its reference: `npx convex deployment select dev/james`",
      "• Select a deployment in another project in the same team: `npx convex deployment select some-project:dev/james`",
      "• Select a deployment in a particular team/project: `npx convex deployment select some-team:some-project:dev/james`",
    ].join("\n"),
  )
  .argument("<deployment>", "The deployment to use")
  .allowExcessArguments(false)
  .action(async (selector) => {
    const ctx = await oneoffContext({
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
    });

    // Get the current deployment selection (no flags, just env/config state)
    const currentSelection = await getDeploymentSelection(ctx, {});

    const parsed = parseDeploymentSelector(selector);
    const isLocalSelector = isLocalDeploymentSelector(parsed);

    // If no project is configured and the selector needs project context, show a specific error
    if (
      currentSelection.kind === "chooseProject" &&
      parsed.kind !== "inTeamProject" &&
      parsed.kind !== "deploymentName" &&
      !isLocalSelector
    ) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `No project configured. Run \`npx convex dev\` to set up a project first, or use a full selector like 'my-team:my-project:dev/james' or 'happy-capybara-123'.`,
      });
    }

    if (isLocalSelector) {
      await handleLocalSelect(ctx, selector, parsed, currentSelection);
      return;
    }

    // Resolve the new deployment using the selector relative to the current project
    const newSelection = await getDeploymentSelection(ctx, {
      url: undefined,
      adminKey: undefined,
      envFile: undefined,
      deployment: selector,
    });

    const deployment = await saveSelectedDeployment(
      ctx,
      selector,
      newSelection,
      deploymentNameFromSelection(currentSelection),
    );
    logFinishedStep("Selected deployment:");
    announceDeploymentTarget(null, deployment);
  });

function isLocalDeploymentSelector(parsed: ParsedDeploymentSelector): boolean {
  return (
    (parsed.kind === "inCurrentProject" ||
      parsed.kind === "inProject" ||
      parsed.kind === "inTeamProject") &&
    parsed.selector.kind === "local"
  );
}

async function handleLocalSelect(
  ctx: Context,
  selector: string,
  parsed: ParsedDeploymentSelector,
  currentSelection: DeploymentSelection,
): Promise<void> {
  const existing = loadProjectLocalConfig(ctx);

  if (existing === null) {
    // No local deployment on disk. Offer to create one (interactive only).
    if (!process.stdin.isTTY) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `No local deployment found. Run ${chalkStderr.bold("npx convex deployment create local")} to create one.`,
      });
    }
    if (
      currentSelection.kind === "chooseProject" &&
      parsed.kind !== "inTeamProject"
    ) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `No project configured. Run \`npx convex dev\` to set up a project first.`,
      });
    }

    // Refusing to create a project if the user didn’t explicitly specify a team.
    if (parsed.kind === "inProject") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `No local deployment found. To create one in ${chalkStderr.bold(parsed.projectSlug)}, run ${chalkStderr.bold(`npx convex deployment create local --project ${parsed.projectSlug}`)}, or use a fully qualified selector like ${chalkStderr.bold(`my-team:${parsed.projectSlug}:local`)}.`,
      });
    }

    const wantsToCreate = await promptYesNo(ctx, {
      message: "No local deployment found. Create one now?",
      default: true,
    });
    if (!wantsToCreate) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `No local deployment found. Run ${chalkStderr.bold("npx convex deployment create local")} to create one.`,
      });
    }
    const teamAndProject = teamAndProjectFromParsed(parsed);
    await createLocalDeployment(ctx, currentSelection, true, teamAndProject);
    return;
  }

  // Resolve the target cloud project the user is asking about
  const target = await targetProjectForLocalSelector(
    ctx,
    parsed,
    currentSelection,
  );

  let resolvedDeploymentName = existing.deploymentName;

  if (target !== null) {
    const match = checkLocalConfigMatchesProject(ctx, existing.config, target);
    if (match === "mismatch") {
      // The on-disk local deployment is tied to a different cloud project.
      // Move it to the new project (warn, pause old, re-register).
      const oldProjectId = existing.config.cloudProjectId!;
      const oldProject = await getCloudProjectSlugsBestEffort(
        ctx,
        oldProjectId,
      );
      const oldProjectLabel =
        oldProject !== null
          ? `project ${chalkStderr.bold(`${oldProject.teamSlug}:${oldProject.slug}`)}`
          : `an unknown cloud project (ID ${oldProjectId})`;
      logWarning(
        chalkStderr.yellow(
          `⚠️ This local deployment was previously in ${oldProjectLabel}. Moving it to project ${chalkStderr.bold(`${target.teamSlug}:${target.slug}`)}.`,
        ),
      );
      await pauseLocalDeploymentBestEffort(ctx, oldProject);
      const { deploymentName: newDeploymentName } = await bigBrainStart(ctx, {
        port: existing.config.ports.cloud,
        teamSlug: target.teamSlug,
        projectSlug: target.slug,
        instanceName: null,
      });
      saveDeploymentConfig(ctx, "local", newDeploymentName, {
        ...existing.config,
        cloudProjectId: target.id,
      });
      resolvedDeploymentName = newDeploymentName;
    } else if (match === "skip") {
      // The on-disk config has no `cloudProjectId` — write the resolved id back
      // so future invocations have it.
      saveDeploymentConfig(ctx, "local", existing.deploymentName, {
        ...existing.config,
        cloudProjectId: target.id,
      });
    }
  }

  const newSelection: DeploymentSelection = {
    kind: "deploymentWithinProject",
    targetProject: {
      kind: "deploymentName",
      deploymentName: resolvedDeploymentName,
      deploymentType: "local",
    },
    selectionWithinProject: {
      kind: "deploymentSelector",
      selector,
    },
  };
  await saveSelectedDeployment(
    ctx,
    selector,
    newSelection,
    deploymentNameFromSelection(currentSelection),
  );
}

function teamAndProjectFromParsed(
  parsed: ParsedDeploymentSelector,
): { teamSlug: string; projectSlug: string } | null {
  if (parsed.kind === "inTeamProject") {
    return { teamSlug: parsed.teamSlug, projectSlug: parsed.projectSlug };
  }
  return null;
}

export async function saveSelectedDeployment(
  ctx: Context,
  selector: string,
  selection: DeploymentSelection,
  previousDeploymentName: string | null,
) {
  const deployment = await loadSelectedDeploymentCredentials(ctx, selection, {
    ensureLocalRunning: false,
  });

  if (deployment.deploymentFields === null) {
    // Should be unreachable since for now, `select` only allows users
    // to select deployments that exist in Big Brain
    return ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: null,
      errForSentry: `Unexpected selection in select: ${JSON.stringify(deployment)}`,
    });
  }

  if (deployment.deploymentFields.deploymentType === "prod") {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Selecting a production deployment is unsupported. To run commands on a production deployment, pass the ${chalkStderr.bold(`--deployment ${selector}`)} flag to each command.`,
    });
  }

  const { convexCloudUrl, convexSiteUrl: siteUrl } =
    deployment.deploymentFields.deploymentType === "local"
      ? { convexCloudUrl: deployment.url, convexSiteUrl: null }
      : await fetchDeploymentCanonicalUrls(ctx, {
          adminKey: deployment.adminKey,
          deploymentUrl: deployment.url,
        });

  await updateEnvAndConfigForDeploymentSelection(
    ctx,
    {
      url: deploymentUrlForApplicationEnv(deployment, convexCloudUrl),
      siteUrl,
      deploymentName: deployment.deploymentFields.deploymentName,
      teamSlug: deployment.deploymentFields.teamSlug,
      projectSlug: deployment.deploymentFields.projectSlug,
      deploymentType: deployment.deploymentFields.deploymentType,
    },
    previousDeploymentName,
  );

  return deployment;
}
