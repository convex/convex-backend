import { Context } from "../../bundler/context.js";
import { logWarning } from "../../bundler/log.js";
import {
  deploymentNameFromAdminKey,
  deploymentTypeFromAdminKey,
  isDeploymentKey,
  isPreviewDeployKey,
  isProjectKey,
} from "./deployment.js";
import { getDeployKeySource } from "./envvars.js";
import { IGNORED_DEPLOY_KEY, readDeployKeyFromEnv } from "./utils/utils.js";

const warnedContexts = new WeakSet<Context>();

export function warnIfDeployKeyOverridesLogin(
  ctx: Context,
  hasAccount: boolean,
) {
  if (
    !hasAccount ||
    warnedContexts.has(ctx) ||
    process.env.CONVEX_OVERRIDE_ACCESS_TOKEN
  )
    return;
  const key = readDeployKeyFromEnv((name) => process.env[name]);
  if (
    key === undefined ||
    key === IGNORED_DEPLOY_KEY ||
    isPreviewDeployKey(key)
  )
    return;
  const { envVarName, source } = getDeployKeySource(key);
  const type = deploymentTypeFromAdminKey(key);
  const target = isDeploymentKey(key)
    ? `${type === "prod" ? "production" : type} deployment "${deploymentNameFromAdminKey(key)}"`
    : isProjectKey(key)
      ? "the project selected by the key"
      : "the deployment selected by the key";
  logWarning(
    `WARNING: ${envVarName} in ${source} overrides your Convex account for deployment commands. They target ${target}. Remove ${envVarName} to use the selected account.`,
  );
  warnedContexts.add(ctx);
}
