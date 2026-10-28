import { chalkStderr } from "chalk";
import { Context } from "../../bundler/context.js";
import { logFinishedStep, logMessage } from "../../bundler/log.js";
import { DeploymentType } from "./api.js";
import { writeUrlsToEnvFile } from "./envvars.js";
import { getDashboardUrl } from "./dashboard.js";

export async function finalizeConfiguration(
  ctx: Context,
  options: {
    functionsPath: string;
    deploymentType: DeploymentType;
    deploymentName: string;
    url: string;
    siteUrl: string | null | undefined;
    wroteToGitIgnore: boolean;
    changedDeploymentEnvVar: boolean;
  },
) {
  const envFileConfig = await writeUrlsToEnvFile(ctx, {
    convexUrl: options.url,
    siteUrl: options.siteUrl,
  });
  const isEnvFileConfigChanged =
    envFileConfig !== null &&
    (envFileConfig.convexUrlEnvVar || envFileConfig.siteUrlEnvVar);

  if (isEnvFileConfigChanged) {
    const deploymentEnvVarMessage = options.changedDeploymentEnvVar
      ? `    name as CONVEX_DEPLOYMENT\n`
      : "";
    const urlUpdateMessages = [];
    if (envFileConfig.convexUrlEnvVar) {
      urlUpdateMessages.push(
        `    client URL as ${envFileConfig.convexUrlEnvVar}\n`,
      );
    }
    if (envFileConfig.siteUrlEnvVar) {
      urlUpdateMessages.push(
        `    HTTP actions URL as ${envFileConfig.siteUrlEnvVar}\n`,
      );
    }
    logFinishedStep(
      `${messageForDeploymentType(options.deploymentType, options.url)} and saved its:\n` +
        deploymentEnvVarMessage +
        urlUpdateMessages.join("") +
        ` to ${envFileConfig.envFile}`,
    );
  } else if (options.changedDeploymentEnvVar) {
    logFinishedStep(
      `${messageForDeploymentType(options.deploymentType, options.url)} and saved its name as CONVEX_DEPLOYMENT to .env.local`,
    );
  }
  if (options.wroteToGitIgnore) {
    logMessage(chalkStderr.gray(`  Added ".env.local" to .gitignore`));
  }
  const anyChanges =
    options.wroteToGitIgnore ||
    options.changedDeploymentEnvVar ||
    isEnvFileConfigChanged;
  if (anyChanges) {
    const dashboardUrl = await getDashboardUrl(ctx, {
      deploymentName: options.deploymentName,
      deploymentType: options.deploymentType,
    });
    logMessage(
      `\nWrite your Convex functions in ${chalkStderr.bold(options.functionsPath)}\n` +
        "Give us feedback at https://convex.dev/community or support@convex.dev\n" +
        `View the Convex dashboard at ${dashboardUrl}\n`,
    );
  }
}

function messageForDeploymentType(deploymentType: DeploymentType, url: string) {
  switch (deploymentType) {
    case "anonymous":
      return `Configured a local deployment for ${url}`;
    case "local":
      return `Configured a local deployment for ${url}`;
    case "dev":
    case "prod":
    case "preview":
    case "custom":
      return `Provisioned a ${deploymentType} deployment`;
    default: {
      deploymentType satisfies never;
      return `Provisioned a ${deploymentType as any} deployment`;
    }
  }
}
