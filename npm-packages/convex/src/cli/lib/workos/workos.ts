/**
 * Programatic provisioning of WorkOS environments and configuration of these environments.
 *
 * This WorkOS integration is subject to change while in development and may require upgrading the CLI
 * to use in the future.
 */
import crypto from "crypto";
import * as dotenv from "dotenv";
import { Context } from "../../../bundler/context.js";
import {
  changeSpinner,
  logFinishedStep,
  logMessage,
  logOutput,
  logVerbose,
  logWarning,
  showSpinner,
  stopSpinner,
} from "../../../bundler/log.js";
import { getTeamAndProjectSlugForDeployment } from "../api.js";
import { deploymentEnvBackend, EnvVarBackend } from "../env.js";
import { deploymentDashboardUrlPage } from "../dashboard.js";
import { changedEnvVarFile, suggestedEnvVarNames } from "../envvars.js";
import { promptOptions, promptYesNo } from "../utils/prompts.js";
import {
  createCORSOrigin,
  createRedirectURI,
  updateAppHomepageUrl,
} from "./environmentApi.js";
import {
  createAssociatedWorkosTeam,
  createEnvironmentAndAPIKey,
  getCandidateEmailsForWorkIntegration,
  getDeploymentCanProvisionWorkOSEnvironments,
} from "./platformApi.js";
import type {
  AuthKitConfig,
  AuthKitEnvironmentConfig,
  AuthKitConfigureSettings,
  ProjectConfig,
} from "../config.js";
import { getAuthKitConfig, readProjectConfig } from "../config.js";

// Helper function to query WorkOS environment variables from a backend
async function getWorkOSEnvVars(backend: EnvVarBackend): Promise<{
  clientId: string | null;
  apiKey: string | null;
  environmentId: string | null;
}> {
  const [clientId, apiKey, environmentId] = await Promise.all([
    backend.get("WORKOS_CLIENT_ID").then((v) => v?.value ?? null),
    backend.get("WORKOS_API_KEY").then((v) => v?.value ?? null),
    backend.get("WORKOS_ENVIRONMENT_ID").then((v) => v?.value ?? null),
  ]);
  return { clientId, apiKey, environmentId };
}

// Helper to resolve WorkOS credentials from all available sources
async function resolveWorkOSCredentials(
  ctx: Context,
  deployment: { deploymentUrl: string; adminKey: string },
  deploymentName: string,
  authKitConfig: AuthKitConfig,
  workosDeploymentType: "dev" | "preview" | "prod",
): Promise<{
  clientId: string | null;
  apiKey: string | null;
  environmentId: string | null;
  deploymentEnvVars: {
    clientId: string | null;
    apiKey: string | null;
    environmentId: string | null;
  };
}> {
  const backend = deploymentEnvBackend(ctx, deployment);
  const deploymentEnvVars = await getWorkOSEnvVars(backend);

  let clientId: string | null;
  let apiKey: string | null;
  let environmentId: string | null;

  if (workosDeploymentType === "dev") {
    // For dev: build environment (.env.local) takes precedence so mismatches
    // with the deployment are caught by ensureDeploymentHasWorkOSCredentials.
    clientId =
      process.env.WORKOS_CLIENT_ID || deploymentEnvVars.clientId || null;
    apiKey = deploymentEnvVars.apiKey || null;
    environmentId =
      process.env.WORKOS_ENVIRONMENT_ID ||
      deploymentEnvVars.environmentId ||
      null;
  } else {
    // For prod/preview: deployment credentials are authoritative. This avoids
    // .env.local dev values overriding prod credentials and triggering a
    // spurious mismatch error during `npx convex deploy`.
    clientId =
      deploymentEnvVars.clientId || process.env.WORKOS_CLIENT_ID || null;
    apiKey = deploymentEnvVars.apiKey || null;
    environmentId =
      deploymentEnvVars.environmentId ||
      process.env.WORKOS_ENVIRONMENT_ID ||
      null;
  }

  // 3. If still no credentials, try provisioning (if we have appropriate auth)
  if (!clientId || !apiKey) {
    const auth = ctx.bigBrainAuth();
    const isUsingDeploymentKey = auth?.kind === "deploymentKey";

    if (isUsingDeploymentKey) {
      await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: buildDeploymentKeyError(
          deploymentName,
          workosDeploymentType,
        ),
      });
    }

    // We have user auth or project key, try to provision
    showSpinner("Provisioning AuthKit environment...");

    try {
      const result = await ensureWorkosEnvironmentProvisioned(
        ctx,
        deploymentName,
        { ...deployment, deploymentNotice: "" },
        authKitConfig,
        workosDeploymentType,
      );

      if (result !== "ready") {
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: "Failed to provision WorkOS environment",
        });
      }

      // After provisioning, re-fetch the credentials
      const provisionedEnvVars = await getWorkOSEnvVars(backend);
      clientId = provisionedEnvVars.clientId;
      apiKey = provisionedEnvVars.apiKey;
      environmentId = provisionedEnvVars.environmentId;

      if (!clientId || !apiKey) {
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage:
            "Failed to retrieve WorkOS credentials after provisioning",
        });
      }
    } catch (error: any) {
      if (
        error.message?.includes("permission") ||
        error.message?.includes("deploy key") ||
        error.message?.includes("UnexpectedAuthHeaderFormat")
      ) {
        await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage:
            `Cannot provision WorkOS environment with current authentication.\n` +
            `You need to manually set WORKOS_CLIENT_ID and WORKOS_API_KEY\n` +
            `environment variables in your build environment or deployment settings.`,
        });
      }
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Error provisioning WorkOS environment: ${error.message}`,
      });
    }
  }

  return { clientId, apiKey, environmentId, deploymentEnvVars };
}

// Helper function to build error message for deployment key restrictions
function buildDeploymentKeyError(
  deploymentName: string,
  deploymentType: string,
): string {
  const integrationsUrl = deploymentDashboardUrlPage(
    deploymentName,
    "/settings/integrations",
  );
  return (
    `AuthKit configuration in convex.json requires WorkOS credentials.\n\n` +
    `Checked for credentials in:\n` +
    `  1. Build environment variables (WORKOS_CLIENT_ID, WORKOS_API_KEY)\n` +
    `  2. Deployment environment variables, see WorkOS integration at ${integrationsUrl}\n\n` +
    `When using a deployment-specific key, you cannot automatically provision WorkOS environments.\n` +
    `You must provide these credentials in your build platform (e.g., Vercel, Netlify)\n` +
    `or set them in your deployment settings.\n\n` +
    `Alternatively, remove the 'authKit.${deploymentType}' section from convex.json to skip\n` +
    `AuthKit configuration.`
  );
}

// Helper function to ensure deployment has the correct WorkOS credentials
async function ensureDeploymentHasWorkOSCredentials(
  ctx: Context,
  deployment: { deploymentUrl: string; adminKey: string },
  credentials: {
    clientId: string;
    apiKey: string;
    environmentId: string | null;
  },
  deploymentEnvVars: {
    clientId: string | null;
    apiKey: string | null;
    environmentId: string | null;
  },
): Promise<void> {
  const mismatches: string[] = [];
  if (
    deploymentEnvVars.clientId &&
    deploymentEnvVars.clientId !== credentials.clientId
  ) {
    mismatches.push(
      `  WORKOS_CLIENT_ID: deployment has '${deploymentEnvVars.clientId}' but we need '${credentials.clientId}'`,
    );
  }
  if (
    deploymentEnvVars.apiKey &&
    deploymentEnvVars.apiKey !== credentials.apiKey
  ) {
    mismatches.push(
      `  WORKOS_API_KEY: deployment has different value than what we need`,
    );
  }
  if (
    deploymentEnvVars.environmentId &&
    credentials.environmentId &&
    deploymentEnvVars.environmentId !== credentials.environmentId
  ) {
    mismatches.push(
      `  WORKOS_ENVIRONMENT_ID: deployment has '${deploymentEnvVars.environmentId}' but we need '${credentials.environmentId}'`,
    );
  }

  if (mismatches.length > 0) {
    await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        `WorkOS environment variable mismatch detected!\n\n` +
        `The following environment variables in your Convex deployment don't match what's needed:\n` +
        mismatches.join("\n") +
        "\n\n" +
        `This would cause your auth configuration to use different credentials at runtime than during build.\n\n` +
        `To fix this, remove the conflicting environment variables from your deployment:\n` +
        `  npx convex env remove WORKOS_CLIENT_ID\n` +
        `  npx convex env remove WORKOS_API_KEY\n` +
        `  npx convex env remove WORKOS_ENVIRONMENT_ID\n\n` +
        `Or remove them from the Convex dashboard deployment settings.\n\n` +
        `Then run your deployment command again.`,
    });
  }
  const updates: Array<{ name: string; value: string }> = [];
  if (!deploymentEnvVars.clientId && credentials.clientId) {
    updates.push({ name: "WORKOS_CLIENT_ID", value: credentials.clientId });
  }
  if (!deploymentEnvVars.apiKey && credentials.apiKey) {
    updates.push({ name: "WORKOS_API_KEY", value: credentials.apiKey });
  }
  if (!deploymentEnvVars.environmentId && credentials.environmentId) {
    updates.push({
      name: "WORKOS_ENVIRONMENT_ID",
      value: credentials.environmentId,
    });
  }

  if (updates.length > 0) {
    changeSpinner("Setting WorkOS credentials in deployment...");
    const backend = deploymentEnvBackend(ctx, deployment);
    await backend.update(updates);
    logVerbose(
      `WorkOS credentials propagated to deployment: ${updates.map((u) => u.name).join(", ")}`,
    );
  }
}

/**
 * Ensure the current deployment has the three expected WorkOS environment
 * variables defined with values corresponding to a valid WorkOS deployment.
 *
 * This may involve provisioning a WorkOS deployment or even (in interactive
 * terminals only) prompting to provision a new WorkOS team to be associated
 * with this Convex team.
 */
export async function ensureWorkosEnvironmentProvisioned(
  ctx: Context,
  deploymentName: string,
  deployment: {
    deploymentUrl: string;
    adminKey: string;
    deploymentNotice: string;
  },
  authKitConfig: AuthKitConfig | undefined,
  deploymentType: "dev" | "preview" | "prod",
): Promise<"ready" | "choseNotToAssociatedTeam"> {
  const envConfig: AuthKitEnvironmentConfig | undefined =
    authKitConfig?.[deploymentType];

  // If no config, nothing to do
  if (!envConfig) {
    return "choseNotToAssociatedTeam";
  }

  showSpinner("Checking for associated AuthKit environment...");
  const backend = deploymentEnvBackend(ctx, deployment);
  const existingEnvVars = await getWorkOSEnvVars(backend);
  if (
    existingEnvVars.clientId &&
    existingEnvVars.environmentId &&
    existingEnvVars.apiKey
  ) {
    logOutput(
      "Deployment already has environment variables for a WorkOS environment configured for AuthKit.",
    );

    if (
      envConfig.localEnvVars !== undefined &&
      envConfig.localEnvVars !== false
    ) {
      await updateEnvLocal(
        ctx,
        existingEnvVars.clientId,
        existingEnvVars.apiKey,
        existingEnvVars.environmentId,
        envConfig.localEnvVars,
      );
    }

    // Configure WorkOS environment if configured
    if (envConfig.configure !== undefined && envConfig.configure !== false) {
      if (!existingEnvVars.apiKey) {
        // API key missing - warn and skip configuration
        logWarning(
          `Skipping WorkOS AuthKit environment configuration: WORKOS_API_KEY is not set.\n` +
            `To configure redirect URIs and CORS origins, you need to set this environment variable.\n` +
            `You can set it at: ${deployment.deploymentUrl.replace(/\/$/, "")}/settings/environment-variables`,
        );
      } else {
        await updateWorkosEnvironment(
          ctx,
          existingEnvVars.apiKey,
          envConfig.configure,
          {
            clientId: existingEnvVars.clientId,
            apiKey: existingEnvVars.apiKey,
            environmentId: existingEnvVars.environmentId,
          },
        );
      }
    }

    logFinishedStep("WorkOS AuthKit environment ready");
    return "ready";
  }

  // We need to provision an environment via Big Brain
  const response = await getDeploymentCanProvisionWorkOSEnvironments(
    ctx,
    deploymentName,
  );
  const { hasAssociatedWorkosTeam, teamId } = response;

  // In case this this becomes a legacy flow that no longer works.
  if ((response as any).disabled) {
    return "choseNotToAssociatedTeam";
  }

  if (!hasAssociatedWorkosTeam) {
    // A WorkOS workspace needs to be created for provisioning to work
    // We'll offer to create it interactively, or fail in non-interactive mode
    const result = await tryToCreateAssociatedWorkosTeam(
      ctx,
      deploymentName,
      teamId,
      deploymentType,
    );
    if (result === "choseNotToAssociatedTeam") {
      return "choseNotToAssociatedTeam";
    }
    result satisfies "ready";
  }

  // Determine WorkOS environment type
  // Map config's "development"/"staging"/"production" to API's "production"/"nonproduction"
  // Default: dev/preview -> nonproduction, prod -> production
  // Override: use environmentType from config (only allowed in prod)
  let workosEnvironmentType: "production" | "nonproduction" | undefined;
  if (envConfig.environmentType) {
    // User explicitly set it (only allowed in prod)
    // Map: "production" -> "production", everything else -> "nonproduction"
    workosEnvironmentType =
      envConfig.environmentType === "production"
        ? "production"
        : "nonproduction";
  } else {
    // Default based on Convex deployment type
    workosEnvironmentType =
      deploymentType === "prod" ? "production" : "nonproduction";
  }

  const environmentResult = await createEnvironmentAndAPIKey(
    ctx,
    deploymentName,
    workosEnvironmentType,
  );

  if (!environmentResult.success) {
    if (
      "error" in environmentResult &&
      environmentResult.error === "team_not_provisioned"
    ) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Team unexpectedly has no provisioned WorkOS team: ${environmentResult.message}`,
      });
    }
    // For other error cases
    const errorMessage =
      "message" in environmentResult
        ? environmentResult.message
        : "Failed to provision WorkOS environment";
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: errorMessage,
    });
  }

  const data = environmentResult.data;
  if (data.newlyProvisioned) {
    logMessage("New AuthKit environment provisioned");
  } else {
    logMessage(
      "Using credentials from existing AuthKit environment already created for this deployment",
    );
  }

  changeSpinner("Setting WORKOS_* deployment environment variables...");
  await setConvexEnvVars(
    backend,
    data.clientId,
    data.environmentId,
    data.apiKey,
  );

  if (
    envConfig.localEnvVars !== undefined &&
    envConfig.localEnvVars !== false
  ) {
    showSpinner("Updating .env.local with WorkOS configuration");
    await updateEnvLocal(
      ctx,
      data.clientId,
      data.apiKey,
      data.environmentId,
      envConfig.localEnvVars,
    );
  }

  // Configure WorkOS environment if configured
  if (envConfig.configure !== undefined && envConfig.configure !== false) {
    await updateWorkosEnvironment(ctx, data.apiKey, envConfig.configure, {
      clientId: data.clientId,
      apiKey: data.apiKey,
      environmentId: data.environmentId,
    });
  }
  logFinishedStep("WorkOS AuthKit environment ready");

  return "ready";
}

/**
 * Interactive flow to provision a WorkOS team for a Convex team.
 * Handles ToS agreement, email selection, and retry logic.
 */
export async function provisionWorkosTeamInteractive(
  ctx: Context,
  deploymentName: string,
  teamId: number,
  deploymentType: "dev" | "preview" | "prod",
  options: {
    promptPrefix?: string;
    promptMessage?: string;
  } = {},
): Promise<
  | { success: true; workosTeamId: string; workosTeamName: string }
  | { success: false; reason: "cancelled" }
> {
  const teamInfo = await getTeamAndProjectSlugForDeployment(ctx, {
    deploymentName,
  });
  if (teamInfo === null) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Can't find Convex Cloud team for deployment ${deploymentName}`,
    });
  }
  stopSpinner();

  const defaultPrefix = `A WorkOS team needs to be created for your Convex team "${teamInfo.teamSlug}" in order to use AuthKit.

You and other members of this team will be able to create WorkOS environments for each Convex dev deployment for projects in this team.

By creating this account you agree to the WorkOS Terms of Service (https://workos.com/legal/terms-of-service) and Privacy Policy (https://workos.com/legal/privacy).
Alternately, choose no and set WORKOS_CLIENT_ID for an existing WorkOS environment.
\n`;

  const defaultMessage = `Create a WorkOS team and enable automatic AuthKit environment provisioning for team "${teamInfo.teamSlug}"?`;

  const agree = await promptYesNo(ctx, {
    prefix: options.promptPrefix ?? defaultPrefix,
    message: options.promptMessage ?? defaultMessage,
    nonInteractiveError: `Cannot provision WorkOS AuthKit in non-interactive mode.

A WorkOS workspace needs to be associated with your Convex team to enable automatic environment provisioning.

To fix this, either:
1. Run this command in an interactive terminal to set up WorkOS provisioning
2. Remove the authKit.${deploymentType} section from convex.json and provide your own WorkOS credentials via the dashboard
3. Set WORKOS_CLIENT_ID and WORKOS_API_KEY environment variables before deploying`,
  });
  if (!agree) {
    logMessage("\nGot it. We won't create your WorkOS account.");
    return { success: false, reason: "cancelled" };
  }

  const alreadyTried = new Map<string, string>();

  let email;
  while (true) {
    let choice = "refresh";
    while (choice === "refresh") {
      const { availableEmails } =
        await getCandidateEmailsForWorkIntegration(ctx);
      choice = await promptOptions<string>(ctx, {
        message:
          availableEmails.length === 1
            ? "Create a new WorkOS team with this email address?"
            : "Create a new WorkOS team with which email address?",
        suffix:
          availableEmails.length === 0
            ? "\nVisit https://dashboard.convex.dev/profile to add a verified email to use to provision a WorkOS account"
            : availableEmails.length === 1
              ? "\nCreate a new WorkOS team with this email address?"
              : "\nTo use another email address visit https://dashboard.convex.dev/profile to add and verify, then choose 'refresh'",
        choices: [
          ...availableEmails.map((email: string) => ({
            name: `${email}${alreadyTried.has(email) ? ` (can't create, a WorkOS team already exists with this email)` : ""}`,
            value: email,
          })),
          {
            name: "refresh (add an email at https://dashboard.convex.dev/profile)",
            value: "refresh",
          } as const,
          {
            name: "cancel (do not create a WorkOS account)",
            value: "cancel",
          } as const,
        ],
      });
    }
    if (choice === "cancel") {
      return { success: false, reason: "cancelled" };
    }
    email = choice;

    const teamResult = await createAssociatedWorkosTeam(ctx, teamId, email);

    if (teamResult.result === "emailAlreadyUsed") {
      logMessage(teamResult.message);
      alreadyTried.set(email, teamResult.message);
      continue;
    }
    // Success!
    return {
      success: true,
      workosTeamId: teamResult.workosTeamId,
      workosTeamName: teamResult.workosTeamName,
    };
  }
}

export async function tryToCreateAssociatedWorkosTeam(
  ctx: Context,
  deploymentName: string,
  teamId: number,
  deploymentType: "dev" | "preview" | "prod",
): Promise<"ready" | "choseNotToAssociatedTeam"> {
  const result = await provisionWorkosTeamInteractive(
    ctx,
    deploymentName,
    teamId,
    deploymentType,
  );

  if (!result.success) {
    logMessage(
      `To provide your own WorkOS environment credentials instead, set environment variables manually:

npx convex env set WORKOS_CLIENT_ID $YOUR_CLIENT_ID_HERE
npx convex env set WORKOS_API_KEY $YOUR_API_KEY_HERE`,
    );
    return "choseNotToAssociatedTeam";
  }

  logFinishedStep("WorkOS team created successfully");
  return "ready";
}

/**
 * Pre-flight check for AuthKit provisioning.
 * Called before building the client bundle to ensure .env.local has correct values.
 * This is the main provisioning path - the error path is kept for backwards compatibility.
 */
/**
 * Ensures WorkOS AuthKit environment is ready before building.
 *
 * Flow:
 * 1. Get authKit configuration for the deployment type
 * 2. Resolve credentials (build env → deployment env → provision via Big Brain)
 * 3. Ensure deployment has the correct credentials
 * 4. Update local .env.local if configured (interactive only)
 * 5. Configure WorkOS environment settings if needed
 */
export async function ensureAuthKitProvisionedBeforeBuild(
  ctx: Context,
  deploymentName: string,
  deployment: {
    deploymentUrl: string;
    adminKey: string;
  },
  deploymentType?: "dev" | "preview" | "prod",
): Promise<void> {
  // 1. Get configuration
  const { projectConfig } = await readProjectConfig(ctx);
  const authKitConfig = await getAuthKitConfig(ctx, projectConfig);
  if (!authKitConfig) {
    return;
  }

  const workosDeploymentType = deploymentType || "dev";
  const envConfig = authKitConfig[workosDeploymentType];
  if (!envConfig) {
    return;
  }

  // 2. Resolve credentials from all sources
  const { clientId, apiKey, environmentId, deploymentEnvVars } =
    await resolveWorkOSCredentials(
      ctx,
      deployment,
      deploymentName,
      authKitConfig,
      workosDeploymentType,
    );

  // 3. Ensure deployment has the correct credentials
  if (clientId && apiKey) {
    await ensureDeploymentHasWorkOSCredentials(
      ctx,
      deployment,
      { clientId, apiKey, environmentId },
      deploymentEnvVars,
    );
  }

  // 4. Update local environment variables if configured (interactive mode only)
  if (envConfig.localEnvVars && process.stdin.isTTY && clientId && apiKey) {
    await updateEnvLocal(
      ctx,
      clientId,
      apiKey,
      environmentId || "",
      envConfig.localEnvVars,
    );
  }

  // 5. Configure WorkOS environment if needed
  if (envConfig.configure && apiKey) {
    const configValues: {
      clientId?: string;
      apiKey?: string;
      environmentId?: string;
    } = {
      apiKey,
    };
    if (clientId) {
      configValues.clientId = clientId;
    }
    if (environmentId) {
      configValues.environmentId = environmentId;
    }
    await updateWorkosEnvironment(
      ctx,
      apiKey,
      envConfig.configure,
      configValues,
    );
  }
}
/**
 * Syncs WorkOS configuration and local env vars after a successful push.
 * This is called on every push in dev mode to keep WorkOS settings in sync
 * with changes to convex.json.
 *
 * @returns true if any updates were made, false if config unchanged
 */
export async function syncAuthKitConfigAfterPush(
  ctx: Context,
  projectConfig: ProjectConfig,
  deployment: {
    deploymentUrl: string;
    adminKey: string;
  },
): Promise<boolean> {
  // Get the authKit config (may include generated defaults for templates)
  const authKitConfig = await getAuthKitConfig(ctx, projectConfig);
  if (!authKitConfig) {
    // No authKit config, nothing to sync
    return false;
  }

  // We only sync the "dev" environment settings during dev mode
  const devConfig = authKitConfig.dev;
  if (!devConfig) {
    return false;
  }

  // Get existing credentials from deployment
  const backend = deploymentEnvBackend(ctx, deployment);
  const { clientId, apiKey, environmentId } = await getWorkOSEnvVars(backend);

  // We need the API key to make WorkOS API calls
  if (!apiKey) {
    // Can't update WorkOS without an API key
    return false;
  }

  // Update WorkOS environment configuration if specified
  if (devConfig.configure !== undefined && devConfig.configure !== false) {
    const provisionedValues: {
      clientId?: string;
      apiKey?: string;
      environmentId?: string;
    } = {
      apiKey,
    };
    if (clientId) {
      provisionedValues.clientId = clientId;
    }
    if (environmentId) {
      provisionedValues.environmentId = environmentId;
    }
    await updateWorkosEnvironment(
      ctx,
      apiKey,
      devConfig.configure,
      provisionedValues,
    );
  }

  // Note: We don't update .env.local during sync - that only happens during provisioning
  // to ensure the client bundle build has the correct values

  return true;
}

// Helpers

async function updateWorkosEnvironment(
  ctx: Context,
  workosApiKey: string,
  configureSettings: AuthKitConfigureSettings,
  provisioned?: { clientId?: string; apiKey?: string; environmentId?: string },
): Promise<void> {
  const isInteractive = process.stdin.isTTY;
  const skippedConfigs: string[] = [];

  // Log what we're about to configure
  const configItems: string[] = [];
  if (configureSettings.redirectUris?.length) {
    configItems.push(
      `${configureSettings.redirectUris.length} redirect URI(s)`,
    );
  }
  if (configureSettings.appHomepageUrl) {
    configItems.push(`app homepage URL`);
  }
  if (configureSettings.corsOrigins?.length) {
    configItems.push(`${configureSettings.corsOrigins.length} CORS origin(s)`);
  }

  if (configItems.length > 0) {
    logVerbose(
      `Starting WorkOS AuthKit configuration: ${configItems.join(", ")}`,
    );
  } else {
    logVerbose(`No WorkOS AuthKit configuration settings to apply`);
    return;
  }

  // Apply each redirect URI
  if (configureSettings.redirectUris) {
    for (const redirectUri of configureSettings.redirectUris) {
      try {
        // Resolve template with both env vars and provisioned values
        const resolvedRedirectUri = resolveTemplate(redirectUri, provisioned);
        const { modified: redirectUriAdded } = await createRedirectURI(
          ctx,
          workosApiKey,
          resolvedRedirectUri,
        );
        if (redirectUriAdded) {
          changeSpinner("Configuring AuthKit redirect URI...");
          logMessage(`AuthKit redirect URI added: ${resolvedRedirectUri}`);
        } else {
          logVerbose(
            `AuthKit redirect URI already configured: ${resolvedRedirectUri}`,
          );
        }
      } catch (error: any) {
        if (
          isInteractive &&
          error.message?.includes("Cannot resolve template")
        ) {
          // In interactive mode, log warning and continue
          skippedConfigs.push(
            `Redirect URI: ${redirectUri} - ${error.message}`,
          );
        } else {
          // In non-interactive mode or for other errors, crash
          return await ctx.crash({
            exitCode: 1,
            errorType: "fatal",
            printedMessage: `Error configuring redirect URI: ${error.message}`,
          });
        }
      }
    }
  }

  // Apply app homepage URL (where users are redirected after logout)
  if (configureSettings.appHomepageUrl) {
    try {
      // Resolve template with both env vars and provisioned values
      const resolvedAppHomepageUrl = resolveTemplate(
        configureSettings.appHomepageUrl,
        provisioned,
      );

      const { modified: appHomepageUrlUpdated } = await updateAppHomepageUrl(
        ctx,
        workosApiKey,
        resolvedAppHomepageUrl,
      );

      if (appHomepageUrlUpdated) {
        changeSpinner("Configuring AuthKit app homepage URL...");
        logMessage(
          `AuthKit app homepage URL updated: ${resolvedAppHomepageUrl}`,
        );
      } else {
        logVerbose(
          `AuthKit app homepage URL was not updated (may be invalid for WorkOS or already set)`,
        );
      }
    } catch (error: any) {
      if (isInteractive && error.message?.includes("Cannot resolve template")) {
        // In interactive mode, log warning and continue
        skippedConfigs.push(
          `App homepage URL: ${configureSettings.appHomepageUrl} - ${error.message}`,
        );
      } else {
        // In non-interactive mode or for other errors, crash
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: `Error configuring app homepage URL: ${error.message}`,
        });
      }
    }
  } else {
    logVerbose(`No app homepage URL configured`);
  }

  // Apply each CORS origin
  if (configureSettings.corsOrigins) {
    for (const corsOrigin of configureSettings.corsOrigins) {
      try {
        // Resolve template with both env vars and provisioned values
        const resolvedCorsOrigin = resolveTemplate(corsOrigin, provisioned);
        const { modified: corsAdded } = await createCORSOrigin(
          ctx,
          workosApiKey,
          resolvedCorsOrigin,
        );
        if (corsAdded) {
          changeSpinner("Configuring AuthKit CORS origin...");
          logMessage(`AuthKit CORS origin added: ${resolvedCorsOrigin}`);
        } else {
          logVerbose(
            `AuthKit CORS origin already configured: ${resolvedCorsOrigin}`,
          );
        }
      } catch (error: any) {
        if (
          isInteractive &&
          error.message?.includes("Cannot resolve template")
        ) {
          // In interactive mode, log warning and continue
          skippedConfigs.push(`CORS origin: ${corsOrigin} - ${error.message}`);
        } else {
          // In non-interactive mode or for other errors, crash
          return await ctx.crash({
            exitCode: 1,
            errorType: "fatal",
            printedMessage: `Error configuring CORS origin: ${error.message}`,
          });
        }
      }
    }
  }

  // Log completion summary
  logVerbose(`WorkOS AuthKit configuration completed`);

  // If we skipped any configurations in interactive mode, let the user know
  if (skippedConfigs.length > 0) {
    stopSpinner();
    logWarning(
      `Skipped some AuthKit configurations due to missing environment variables:\n` +
        skippedConfigs.map((s) => `  - ${s}`).join("\n"),
    );
  }
}

// Helper to resolve template strings with unified syntax:
// - ${buildEnv.ENV_VAR} for build-time environment variables
// - ${authEnv.WORKOS_CLIENT_ID} for provisioned client ID
// - ${authEnv.WORKOS_API_KEY} for provisioned API key
// - ${authEnv.WORKOS_ENVIRONMENT_ID} for provisioned environment ID
/* eslint-disable no-restricted-syntax */
export function resolveTemplate(
  str: string,
  provisioned?: { clientId?: string; apiKey?: string; environmentId?: string },
): string {
  return str.replace(/\$\{([^}]+)\}/g, (match, expression) => {
    // Handle auth environment values (provisioned WorkOS credentials)
    if (expression === "authEnv.WORKOS_CLIENT_ID") {
      if (!provisioned?.clientId) {
        throw new Error(
          `Cannot resolve template ${match}: WORKOS_CLIENT_ID not available. ` +
            `Ensure WorkOS environment is provisioned.`,
        );
      }
      return provisioned.clientId;
    }
    if (expression === "authEnv.WORKOS_API_KEY") {
      if (!provisioned?.apiKey) {
        throw new Error(
          `Cannot resolve template ${match}: WORKOS_API_KEY not available. ` +
            `Ensure WorkOS environment is provisioned.`,
        );
      }
      return provisioned.apiKey;
    }
    if (expression === "authEnv.WORKOS_ENVIRONMENT_ID") {
      if (!provisioned?.environmentId) {
        throw new Error(
          `Cannot resolve template ${match}: WORKOS_ENVIRONMENT_ID not available. ` +
            `Ensure WorkOS environment is provisioned.`,
        );
      }
      return provisioned.environmentId;
    }

    // Handle build environment variables
    if (expression.startsWith("buildEnv.")) {
      const varName = expression.substring("buildEnv.".length);
      const value = process.env[varName];
      if (!value) {
        throw new Error(
          `Cannot resolve template ${match}: Environment variable ${varName} is not set.`,
        );
      }
      return value;
    }

    // Unknown template expression - fail loudly
    throw new Error(
      `Unknown template expression: ${match}. ` +
        `Use \${buildEnv.VAR_NAME} for environment variables or ` +
        `\${authEnv.WORKOS_CLIENT_ID/WORKOS_API_KEY} for provisioned values.`,
    );
  });
}
/* eslint-enable no-restricted-syntax */

// Update .env.local based on configured localEnvVars
async function updateEnvLocal(
  ctx: Context,
  clientId: string,
  apiKey: string,
  environmentId: string,
  localEnvVarsConfig: Record<string, string>,
) {
  const envPath = ".env.local";

  let existingFileContent = ctx.fs.exists(envPath)
    ? ctx.fs.readUtf8File(envPath)
    : null;

  // Build the changes based on localEnvVarsConfig
  let suggestedChanges: Record<
    string,
    {
      value: string;
      commentAfterValue?: string;
      commentOnPreviousLine?: string;
    }
  > = {};

  const { detectedFramework } = await suggestedEnvVarNames(ctx);

  // Parse existing .env.local to check what's already there
  const existingEnvVars = existingFileContent
    ? dotenv.parse(existingFileContent)
    : {};

  for (const [envVarName, templateValue] of Object.entries(
    localEnvVarsConfig,
  )) {
    // Check if already set in .env.local
    if (existingEnvVars[envVarName]) {
      logVerbose(`Skipping ${envVarName} update - already in .env.local`);
      continue;
    }

    // Check if already set in environment (but not from .env.local)
    if (process.env[envVarName]) {
      logVerbose(
        `Skipping ${envVarName} update in .env.local - already set in environment`,
      );
      continue;
    }

    // Use unified template resolution for both syntaxes
    const resolvedValue = resolveTemplate(templateValue, {
      clientId,
      apiKey,
      environmentId,
    });

    // Add comment for first WorkOS var if it's a provisioned value
    if (
      Object.keys(suggestedChanges).length === 0 &&
      (templateValue.includes("authEnv.WORKOS_CLIENT_ID") ||
        templateValue === "${authEnv.WORKOS_CLIENT_ID}")
    ) {
      suggestedChanges[envVarName] = {
        value: resolvedValue,
        commentOnPreviousLine: `# See this environment at ${workosUrl(environmentId, "/authentication")}`,
      };
    } else {
      suggestedChanges[envVarName] = { value: resolvedValue };
    }
  }

  // Special handling for WORKOS_COOKIE_PASSWORD for Next.js/TanStackStart
  if (
    (detectedFramework === "Next.js" ||
      detectedFramework === "TanStackStart") &&
    !process.env["WORKOS_COOKIE_PASSWORD"] && // Don't override environment
    (!existingFileContent ||
      !existingFileContent.includes("WORKOS_COOKIE_PASSWORD"))
  ) {
    suggestedChanges["WORKOS_COOKIE_PASSWORD"] = {
      value: crypto.randomBytes(32).toString("base64url"),
    };
  }

  for (const [
    envVarName,
    { value: envVarValue, commentOnPreviousLine, commentAfterValue },
  ] of Object.entries(suggestedChanges) as [
    string,
    {
      value: string;
      commentOnPreviousLine?: string;
      commentAfterValue?: string;
    },
  ][]) {
    existingFileContent =
      changedEnvVarFile({
        existingFileContent,
        envVarName,
        envVarValue,
        commentAfterValue: commentAfterValue ?? null,
        commentOnPreviousLine: commentOnPreviousLine ?? null,
      }) || existingFileContent;
  }

  if (
    existingFileContent !== null &&
    Object.keys(suggestedChanges).length > 0
  ) {
    ctx.fs.writeUtf8File(envPath, existingFileContent);
    logMessage(
      `Updated .env.local with ${Object.keys(suggestedChanges).join(", ")}`,
    );
  }
}

async function setConvexEnvVars(
  backend: EnvVarBackend,
  workosClientId: string,
  workosEnvironmentId: string,
  workosEnvironmentApiKey: string,
) {
  await backend.update([
    { name: "WORKOS_CLIENT_ID", value: workosClientId },
    { name: "WORKOS_ENVIRONMENT_ID", value: workosEnvironmentId },
    { name: "WORKOS_API_KEY", value: workosEnvironmentApiKey },
  ]);
}

type Subpaths = "/authentication" | "/sessions" | "/redirects" | "/users";
function workosUrl(environmentId: string, subpath: Subpaths) {
  return `https://dashboard.workos.com/${environmentId}${subpath}`;
}
