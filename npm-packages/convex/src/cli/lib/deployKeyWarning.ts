import { chalkStderr } from "chalk";
import * as dotenv from "dotenv";
import { Context } from "../../bundler/context.js";
import { logWarning } from "../../bundler/log.js";
import {
  deploymentTypeFromAdminKey,
  isDeploymentKey,
  isPreviewDeployKey,
  isProjectKey,
} from "./deployment.js";
import { isAuthorizedHeader } from "./login.js";
import {
  CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
  CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME,
  ENV_VAR_FILE_PATH,
  processDeployKeyValue,
  readDeployKeyFromEnv,
} from "./utils/utils.js";

// A deploy key silently outranks the account a developer logged in with, so
// warn about it once per command, as loudly as the command allows.
let warnedThisProcess = false;

type DeployKeyInEnv = {
  key: string;
  envVarName: string;
  // The env file the key came from, or "shell environment".
  source: string;
};

/**
 * The env file a deploy key came from, or null if it came from the shell.
 * `dotenv` doesn't overwrite variables that are already set, so a value from
 * the shell outranks these files -- match on the value to tell them apart.
 */
function deployKeyEnvFile(
  ctx: Context,
  envVarName: string,
  value: string,
): string | null {
  for (const file of [ENV_VAR_FILE_PATH, ".env"]) {
    if (!ctx.fs.exists(file)) {
      continue;
    }
    if (dotenv.parse(ctx.fs.readUtf8File(file))[envVarName] === value) {
      return file;
    }
  }
  return null;
}

async function readDeployKeyInEnv(
  ctx: Context,
): Promise<DeployKeyInEnv | null> {
  const key = await processDeployKeyValue(
    ctx,
    readDeployKeyFromEnv((name) => process.env[name]),
  );
  if (key === undefined) {
    return null;
  }
  const envVarName = process.env[CONVEX_DEPLOY_KEY_ENV_VAR_NAME]
    ? CONVEX_DEPLOY_KEY_ENV_VAR_NAME
    : CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME;
  return {
    key,
    envVarName,
    source: deployKeyEnvFile(ctx, envVarName, key) ?? "shell environment",
  };
}

function describeKey(key: string): {
  target: string;
  effect: string;
  isProd: boolean;
} {
  if (isPreviewDeployKey(key)) {
    return {
      target: "preview deploy key",
      effect:
        "`npx convex deploy` creates preview deployments for this key's project.",
      isProd: false,
    };
  }
  if (isProjectKey(key)) {
    return {
      target: "project key",
      effect: "Commands create and manage deployments in this key's project.",
      isProd: false,
    };
  }
  if (isDeploymentKey(key)) {
    const deploymentType = deploymentTypeFromAdminKey(key);
    const deploymentName = key.slice(0, key.indexOf("|")).split(":")[1];
    const isProd = deploymentType === "prod";
    return {
      target: `${isProd ? "PRODUCTION" : deploymentType} deployment "${deploymentName}"`,
      effect: `\`dev\`, \`deploy\`, \`run\`, \`env\`, \`logs\`, \`data\`, \`import\` and \`export\` act on this deployment only.${
        isProd
          ? " `npx convex dev` and `npx convex run` change production."
          : ""
      }`,
      isProd,
    };
  }
  return {
    target: "deploy key",
    effect: "Commands act on the deployment this key belongs to.",
    isProd: false,
  };
}

/**
 * Print a banner describing the deploy key in effect for this directory, and
 * that it overrides the Convex account. Used by commands about logging in,
 * where the difference matters most.
 */
export async function printDeployKeyBanner(ctx: Context): Promise<boolean> {
  const found = await readDeployKeyInEnv(ctx);
  if (found === null) {
    return false;
  }
  warnedThisProcess = true;
  const { key, envVarName, source } = found;
  // Big Brain only strips a `project:`/`team:` prefix itself, so a deployment
  // key authorizes only without its `dev:`/`prod:` prefix. Split on the first
  // `|` like the server does, so a secret containing one survives.
  const secret = key.slice(key.indexOf("|") + 1);
  const isValid = await isAuthorizedHeader(ctx, `Bearer ${secret}`);
  const { target, effect, isProd } = describeKey(key);
  const color = isProd ? chalkStderr.red : chalkStderr.yellow;
  const rule = color("━".repeat(72));
  logWarning(
    [
      rule,
      color.bold(
        `WARNING: ${envVarName} OVERRIDES YOUR CONVEX LOGIN IN THIS DIRECTORY`,
      ),
      rule,
      `Source:   ${source}`,
      `Key:      ${target} (${isValid ? "valid" : color("invalid or expired")})`,
      `Effect:   ${effect}`,
      `Ignored:  the account you are logged in with, and any account set for`,
      `          this directory with \`npx convex account use\`.`,
      `To use your account instead, remove ${envVarName} from ${source}.`,
      rule,
    ].join("\n"),
  );
  return true;
}

/**
 * One-line warning for other commands when a deploy key outranks an account
 * this device is logged in with. Machines with only a deploy key (such as CI)
 * see nothing.
 */
export async function warnIfDeployKeyOverridesLogin(
  ctx: Context,
  isLoggedIn: boolean,
) {
  if (warnedThisProcess || !isLoggedIn) {
    return;
  }
  const auth = ctx.bigBrainAuth();
  if (auth === null || auth.kind === "accessToken") {
    return;
  }
  const found = await readDeployKeyInEnv(ctx);
  if (found === null) {
    return;
  }
  warnedThisProcess = true;
  const { target, isProd } = describeKey(found.key);
  const color = isProd ? chalkStderr.red : chalkStderr.yellow;
  logWarning(
    color.bold(
      `WARNING: ${found.envVarName} from ${found.source} overrides your Convex login: this command uses the ${target}. Run \`npx convex login status\` for details.`,
    ),
  );
}
