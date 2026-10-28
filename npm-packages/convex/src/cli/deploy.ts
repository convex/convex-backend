import { chalkStderr } from "chalk";
import { Command, Option } from "@commander-js/extra-typings";
import { Context, oneoffContext } from "../bundler/context.js";
import { logFinishedStep, logMessage, showSpinner } from "../bundler/log.js";
import { loadSelectedDeploymentCredentials } from "./lib/api.js";
import { announceDeploymentTarget } from "./lib/announceDeploymentTarget.js";
import {
  getDefaultDeployMessage,
  gitBranchFromEnvironment,
  isNonProdBuildEnvironment,
  suggestedEnvVarNames,
} from "./lib/envvars.js";
import { PushOptions } from "./lib/components.js";
import {
  CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
  CONVEX_SELF_HOSTED_URL_VAR_NAME,
  CONVEX_DEPLOYMENT_ENV_VAR_NAME,
  bigBrainAPI,
} from "./lib/utils/utils.js";
import { runFunctionAndLog } from "./lib/run.js";
import { usageStateWarning } from "./lib/usage.js";
import { getTeamAndProjectFromPreviewAdminKey } from "./lib/deployment.js";
import { runPush } from "./lib/components.js";
import { promptYesNo } from "./lib/utils/prompts.js";
import { deployToDeployment, runCommand } from "./lib/deploy2.js";
import {
  DeploymentSelection,
  getDeploymentSelection,
} from "./lib/deploymentSelection.js";
import { deploymentNameAndTypeFromSelection } from "./lib/deploymentSelection.js";
import { checkVersionAndAiFilesStaleness } from "./lib/updates.js";
import { readProjectConfig, getAuthKitConfig } from "./lib/config.js";
import { ensureAuthKitProvisionedBeforeBuild } from "./lib/workos/workos.js";
import { DASHBOARD_HOST } from "./lib/dashboard.js";
import { extractDeploymentNameForWorkOS } from "./lib/extractDeploymentNameForWorkOS.js";

export const deploy = new Command("deploy")
  .summary("Deploy to a production or preview deployment")
  .description(
    [
      "Deploys code to a deployment.",
      "This is typically used for deploying to a prod or preview deployment manually or from CI; to deploy to your dev deployment when developing, use `npx convex dev`.",
      "",
      "The target deployment is chosen like this:",
      `• If the \`${CONVEX_DEPLOYMENT_ENV_VAR_NAME}\` environment variable is set (typical during local development), the target is the project’s default production deployment.`,
      `• If the \`${CONVEX_DEPLOY_KEY_ENV_VAR_NAME}\` environment variable is set (typical in CI), it is the deployment associated with that key.`,
      `  • When it’s set to a preview deploy key, it will deploy to a preview deployment:`,
      `    • with the name of the current Git branch when running in CI (Vercel, Netlify, Cloudflare Pages, GitHub)`,
      `    • or with the name specified by the \`--preview-name\` or \`--preview-create\` flags`,
      "",
      "`npx convex deploy` will:",
      "  1. Run a command if specified with `--cmd`, with the deployment URL available as an environment variable.",
      "  2. Typecheck your Convex functions.",
      "  3. Regenerate the generated code in the `convex/_generated` directory.",
      "  4. Bundle your Convex functions and their dependencies.",
      "  5. Push your functions, indexes, and schema to the deployment.",
      "  6. When deploying to a preview deployment, it runs the function specified by `--preview-run`.",
      "If any step fails, the next steps do not run.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .addDeployOptions()
  .addOption(
    new Option(
      "--preview-run <functionName>",
      "Function to run if deploying to a preview deployment. This is ignored if deploying to a production deployment.",
    ),
  )
  .addOption(
    new Option(
      "--preview-name <name>",
      "The name to associate with this preview deployment. Defaults to the current Git branch name in Vercel, Netlify, Cloudflare Pages and GitHub CI. Reuses the existing deployment if one exists.",
    ).conflicts("preview-create"),
  )
  .addOption(
    new Option(
      "--preview-create <name>",
      "Like --preview-name, but deletes and recreates an existing preview deployment with the same name. This parameter can only be used with a preview deploy key (when used with another type of key, the command will return an error).",
    ).conflicts("preview-name"),
  )
  .addOption(
    new Option(
      "--check-build-environment <mode>",
      "Whether to check for a non-production build environment before deploying to a production Convex deployment.",
    )
      .choices(["enable", "disable"] as const)
      .default("enable" as const)
      .hideHelp(),
  )
  // Hidden options to pass in admin key and url for tests and local development
  .addOption(new Option("--admin-key <adminKey>").hideHelp())
  .addOption(new Option("--url <url>").hideHelp())
  .addOption(
    new Option(
      "--env-file <envFile>",
      `Path to a custom file of environment variables, for choosing the \
deployment, e.g. ${CONVEX_DEPLOYMENT_ENV_VAR_NAME} or ${CONVEX_SELF_HOSTED_URL_VAR_NAME}. \
Same format as .env.local or .env files, and overrides them.`,
    ),
  )
  .addOption(
    new Option(
      "--message <message>",
      "Optional message to attach to this deployment in the audit log.",
    ),
  )
  .addOption(
    new Option(
      "--skip-workos-check",
      "Skip WorkOS AuthKit provisioning and credential checks during deploy.",
    ).hideHelp(),
  )
  .addOption(
    new Option(
      "--skip-large-indexes-check",
      "Skip the confirmation prompt when this push creates, changes, or deletes an index on a large table. Creating or changing an index blocks the deploy until it is backfilled; consider staging it instead so it backfills in the background.",
    ),
  )
  .addOption(
    // Predecessor of --skip-large-indexes-check, kept hidden so existing
    // deploy scripts keep working. It still skips only the deletion
    // confirmation: scripts that opted into that never agreed to let a
    // blocking backfill through.
    new Option("--allow-deleting-large-indexes").hideHelp(),
  )
  .addOption(
    new Option(
      "--allow-unstaging-schema",
      "Skip the confirmation prompt when this push discards a staged table validator that is validating or already validated.",
    ),
  )
  .showHelpAfterError()
  .action(async (cmdOptions) => {
    const ctx = await oneoffContext(cmdOptions);

    const deploymentSelection = await getDeploymentSelection(ctx, {
      ...cmdOptions,
      implicitProd: true,
    });
    if (
      cmdOptions.checkBuildEnvironment === "enable" &&
      isNonProdBuildEnvironment() &&
      deploymentSelection.kind === "existingDeployment" &&
      deploymentSelection.deploymentToActOn.source === "deployKey" &&
      deploymentSelection.deploymentToActOn.deploymentFields?.deploymentType ===
        "prod"
    ) {
      await ctx.crash({
        exitCode: 1,
        errorType: "invalid filesystem data",
        printedMessage: `Detected a non-production build environment and "${CONVEX_DEPLOY_KEY_ENV_VAR_NAME}" for a production Convex deployment.\n
          This is probably unintentional.
          `,
      });
    }

    if (deploymentSelection.kind === "anonymous") {
      logMessage(
        "You are currently developing anonymously with a locally running project.\n" +
          "To deploy your Convex app to the cloud, log in by running `npx convex login`.\n" +
          "See https://docs.convex.dev/production for more information on how Convex cloud works and instructions on how to set up hosting.",
      );
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: null,
      });
    }

    if (deploymentSelection.kind === "preview") {
      // TODO -- add usage state warnings here too once we can do it without a deployment name
      // await usageStateWarning(ctx);
      const previewName =
        cmdOptions.previewCreate ??
        cmdOptions.previewName ??
        gitBranchFromEnvironment();
      const reuse = cmdOptions.previewCreate === undefined;

      const teamAndProjectSlugs = await getTeamAndProjectFromPreviewAdminKey(
        ctx,
        deploymentSelection.previewDeployKey,
      );
      await deployToNewPreviewDeployment(
        ctx,
        {
          previewDeployKey: deploymentSelection.previewDeployKey,
          projectSelection: {
            kind: "teamAndProjectSlugs",
            teamSlug: teamAndProjectSlugs.teamSlug,
            projectSlug: teamAndProjectSlugs.projectSlug,
          },
        },
        {
          ...cmdOptions,
          previewName: previewName ?? undefined,
          reuse,
          skipLargeIndexesCheck: cmdOptions.skipLargeIndexesCheck ?? false,
          message: cmdOptions.message ?? getDefaultDeployMessage(),
        },
      );
    } else {
      if (cmdOptions.previewCreate !== undefined) {
        const source =
          deploymentSelection.kind === "deploymentWithinProject" &&
          deploymentSelection.targetProject.kind === "deploymentName"
            ? `at ${chalkStderr.blue.underline(`${DASHBOARD_HOST}/dp/${deploymentSelection.targetProject.deploymentName}/settings#preview-deploy-keys`)}`
            : deploymentSelection.kind === "existingDeployment" &&
                deploymentSelection.deploymentToActOn.deploymentFields !== null
              ? `at ${chalkStderr.blue.underline(`${DASHBOARD_HOST}/dp/${deploymentSelection.deploymentToActOn.deploymentFields.deploymentName}/settings#preview-deploy-keys`)}`
              : "on the dashboard";
        await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: `Preview deployments can only be created with preview deploy keys. Generate a preview deploy key ${source} and set the ${chalkStderr.bold(`CONVEX_DEPLOY_KEY`)} environment variable with it.`,
        });
      }

      await deployToExistingDeployment(ctx, deploymentSelection, {
        ...cmdOptions,
        skipWorkosCheck: cmdOptions.skipWorkosCheck ?? false,
        skipLargeIndexesCheck: cmdOptions.skipLargeIndexesCheck ?? false,
        allowDeletingLargeIndexes:
          cmdOptions.allowDeletingLargeIndexes ?? false,
        allowUnstagingSchema: cmdOptions.allowUnstagingSchema ?? false,
        message: cmdOptions.message ?? getDefaultDeployMessage(),
      });
    }
  });

async function deployToNewPreviewDeployment(
  ctx: Context,
  deploymentSelection: {
    previewDeployKey: string;
    projectSelection: {
      kind: "teamAndProjectSlugs";
      teamSlug: string;
      projectSlug: string;
    };
  },
  options: {
    dryRun?: boolean | undefined;
    previewName?: string | undefined;
    reuse: boolean;
    previewRun?: string | undefined;
    cmdUrlEnvVarName?: string | undefined;
    cmd?: string | undefined;
    verbose?: boolean | undefined;
    typecheck: "enable" | "try" | "disable";
    typecheckComponents: boolean;
    codegen: "enable" | "disable";
    pushAllModules?: boolean;

    debug?: boolean | undefined;
    debugBundlePath?: string | undefined;
    skipWorkosCheck?: boolean | undefined;
    skipLargeIndexesCheck: boolean;
    message: string | null;
  },
) {
  const previewName = options.previewName ?? null;
  if (previewName === null) {
    await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        "`npx convex deploy` to a preview deployment could not determine the preview name. Provide one using `--preview-name`",
    });
  }

  if (options.dryRun) {
    logFinishedStep(
      `Would have claimed preview deployment for "${previewName}"`,
    );
    await runCommand(ctx, {
      cmdUrlEnvVarName: options.cmdUrlEnvVarName,
      cmd: options.cmd,
      dryRun: !!options.dryRun,
      url: "https://<PREVIEW DEPLOYMENT>.convex.cloud",
      adminKey: "preview-deployment-admin-key",
    });
    logFinishedStep(
      `Would have deployed Convex functions to preview deployment for "${previewName}"`,
    );
    if (options.previewRun !== undefined) {
      logMessage(`Would have run function "${options.previewRun}"`);
    }
    return;
  }
  const data = await bigBrainAPI({
    ctx,
    method: "POST",
    path: "claim_preview_deployment",
    data: {
      projectSelection: deploymentSelection.projectSelection,
      identifier: previewName,
      reuse: options.reuse,
    },
  });

  const previewAdminKey = data.adminKey;
  const previewUrl = data.instanceUrl;

  // Extract deployment name from URL for WorkOS provisioning
  const deploymentNameForWorkOS = extractDeploymentNameForWorkOS(previewUrl);

  announceDeploymentTarget("Deploying code to deployment:", {
    url: previewUrl,
    deploymentFields: {
      deploymentName: data.deploymentName,
      deploymentType: "preview",
      teamSlug: deploymentSelection.projectSelection.teamSlug,
      projectSlug: deploymentSelection.projectSelection.projectSlug,
      reference: data.reference,
      // Preview deployments are never default
      isDefault: false,
    },
  });

  // Provision WorkOS before building the client bundle (if configured)
  const { projectConfig } = await readProjectConfig(ctx);
  const authKitConfig = await getAuthKitConfig(ctx, projectConfig);

  if (authKitConfig && deploymentNameForWorkOS && !options.skipWorkosCheck) {
    await ensureAuthKitProvisionedBeforeBuild(
      ctx,
      deploymentNameForWorkOS,
      { deploymentUrl: previewUrl, adminKey: previewAdminKey },
      "preview",
    );
  }

  await runCommand(ctx, {
    ...options,
    url: previewUrl,
    adminKey: previewAdminKey,
  });

  const pushOptions: PushOptions = {
    deploymentName: data.deploymentName,
    deploymentType: "preview",
    adminKey: previewAdminKey,
    verbose: !!options.verbose,
    dryRun: false,
    typecheck: options.typecheck,
    typecheckComponents: options.typecheckComponents,
    debug: !!options.debug,
    debugBundlePath: options.debugBundlePath,
    debugNodeApis: false,
    codegen: options.codegen === "enable",
    url: previewUrl,
    liveComponentSources: false,
    pushAllModules: !!options.pushAllModules,
    largeIndexDeletionCheck: "no verification", // fine for preview deployments
    // A freshly claimed preview has no data to backfill. A reused one keeps
    // the data earlier pushes wrote, so an unstaged index there can block
    // the deploy like on any other deployment.
    largeIndexBackfillCheck: data.isNewDeployment
      ? "no verification"
      : options.skipLargeIndexesCheck
        ? "has confirmation"
        : "ask for confirmation",
    // Staged validation on throwaway preview data is cheap to redo, so
    // discarding it needs no confirmation.
    stagedValidatorDeletionCheck: "no verification",
    warnOnSlowSchemaValidation: true,
    message: options.message,
  };
  showSpinner(`Deploying to ${previewUrl}...`);
  await runPush(ctx, pushOptions);
  logFinishedStep(`Deployed Convex functions to ${previewUrl}`);

  if (options.previewRun !== undefined && data.isNewDeployment) {
    await runFunctionAndLog(ctx, {
      deploymentUrl: previewUrl,
      adminKey: previewAdminKey,
      functionName: options.previewRun,
      argsString: "{}",
      componentPath: undefined,
      callbacks: {
        onSuccess: () => {
          logFinishedStep(`Finished running function "${options.previewRun}"`);
        },
      },
    });
  }
}

async function deployToExistingDeployment(
  ctx: Context,
  deploymentSelection: DeploymentSelection,
  options: {
    verbose?: boolean | undefined;
    dryRun?: boolean | undefined;
    yes?: boolean | undefined;
    typecheck: "enable" | "try" | "disable";
    typecheckComponents: boolean;
    codegen: "enable" | "disable";
    cmd?: string | undefined;
    cmdUrlEnvVarName?: string | undefined;
    pushAllModules?: boolean;

    debugBundlePath?: string | undefined;
    debug?: boolean | undefined;
    adminKey?: string | undefined;
    url?: string | undefined;
    writePushRequest?: string | undefined;
    liveComponentSources?: boolean | undefined;
    envFile?: string | undefined;
    skipWorkosCheck?: boolean | undefined;
    skipLargeIndexesCheck: boolean;
    allowDeletingLargeIndexes: boolean;
    allowUnstagingSchema: boolean;
    message: string | null;
  },
) {
  const deploymentToActOn = await loadSelectedDeploymentCredentials(
    ctx,
    deploymentSelection,
  );
  announceDeploymentTarget("Deploying code to deployment:", deploymentToActOn);
  const { deploymentFields } = deploymentToActOn;

  const configuredDeployment =
    deploymentNameAndTypeFromSelection(deploymentSelection);
  if (configuredDeployment !== null && configuredDeployment.name !== null) {
    const shouldPushToProd =
      configuredDeployment.name === deploymentFields?.deploymentName ||
      (options.yes ??
        (await askToConfirmPush(
          ctx,
          {
            configuredName: configuredDeployment.name,
            configuredType: configuredDeployment.type,
            requestedName: deploymentFields?.deploymentName!,
            requestedType: deploymentFields?.deploymentType!,
          },
          deploymentToActOn.url,
        )));
    if (!shouldPushToProd) {
      await ctx.crash({
        exitCode: 1,
        printedMessage: null,
        errorType: "fatal",
      });
    }
  }

  const isCloudDeployment = deploymentFields !== null;
  await Promise.all([
    deployToDeployment(
      ctx,
      {
        url: deploymentToActOn.url,
        adminKey: deploymentToActOn.adminKey,
        deploymentName: deploymentFields?.deploymentName ?? null,
        ...(deploymentFields?.deploymentType !== undefined
          ? { deploymentType: deploymentFields.deploymentType }
          : {}),
      },
      { ...options, skipWorkosCheck: options.skipWorkosCheck },
    ),
    ...(isCloudDeployment
      ? [
          usageStateWarning(ctx, deploymentFields.deploymentName),
          checkVersionAndAiFilesStaleness(ctx),
        ]
      : []),
  ]);
}

async function askToConfirmPush(
  ctx: Context,
  deployment: {
    configuredName: string;
    configuredType: string | null;
    requestedName: string;
    requestedType: string;
  },
  prodUrl: string,
) {
  logMessage(
    `\
You're currently developing against your ${chalkStderr.bold(
      deployment.configuredType ?? "dev",
    )} deployment

  ${deployment.configuredName} (set in CONVEX_DEPLOYMENT)

Your ${chalkStderr.bold(deployment.requestedType)} deployment ${chalkStderr.bold(
      deployment.requestedName,
    )} serves traffic at:

  ${(await suggestedEnvVarNames(ctx)).convexUrlEnvVar}=${chalkStderr.bold(prodUrl)}

Make sure that your published client is configured with this URL (for instructions see https://docs.convex.dev/hosting)\n`,
  );
  return promptYesNo(ctx, {
    message: `Do you want to push your code to your ${deployment.requestedType} deployment ${deployment.requestedName} now?`,
    default: true,
  });
}
