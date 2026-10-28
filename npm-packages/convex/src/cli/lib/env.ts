import { chalkStderr } from "chalk";
import * as dotenv from "dotenv";
import { Context } from "../../bundler/context.js";
import {
  logFailure,
  logFinishedStep,
  logMessage,
  logOutput,
} from "../../bundler/log.js";
import { runSystemQuery } from "./run.js";
import { deploymentFetch, logAndHandleFetchError } from "./utils/utils.js";
import { readFromStdin } from "./utils/stdin.js";
import { promptSecret } from "./utils/prompts.js";
import { detectAiAgent } from "./utils/aiAgent.js";
import {
  EXPECTED_CONVEX_URL_NAMES,
  EXPECTED_SITE_URL_NAMES,
} from "./envvars.js";
import { formatEnvValueForDotfile } from "./formatEnvValueForDotfile.js";
import {
  CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
  CONVEX_DEPLOYMENT_ENV_VAR_NAME,
  CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME,
  CONVEX_SELF_HOSTED_URL_VAR_NAME,
  CONVEX_SELF_HOSTED_ADMIN_KEY_VAR_NAME,
} from "./utils/utils.js";

function formatList(items: string[]): string {
  if (items.length === 0) return "";
  if (items.length === 1) return items[0];
  if (items.length === 2) return `${items[0]} and ${items[1]}`;
  return `${items.slice(0, -1).join(", ")}, and ${items[items.length - 1]}`;
}

export type EnvVarChange = {
  name: string;
  value: string | null; // null = delete
};

export type EnvVar = {
  name: string;
  value: string;
};

export interface EnvVarBackend {
  get(name: string): Promise<EnvVar | null>;
  list(): Promise<EnvVar[]>;
  update(changes: EnvVarChange[]): Promise<void>;
  notice: string;
}

export function deploymentEnvBackend(
  ctx: Context,
  deployment: {
    deploymentUrl: string;
    adminKey: string;
    deploymentNotice?: string;
  },
): EnvVarBackend {
  return {
    async get(name) {
      const envVar = (await runSystemQuery(ctx, {
        ...deployment,
        functionName: "_system/cli/queryEnvironmentVariables:get",
        componentPath: undefined,
        args: { name },
      })) as EnvVar | null;
      return envVar;
    },
    async list() {
      return (await runSystemQuery(ctx, {
        ...deployment,
        functionName: "_system/cli/queryEnvironmentVariables",
        componentPath: undefined,
        args: {},
      })) as EnvVar[];
    },
    async update(changes) {
      const fetch = deploymentFetch(ctx, deployment);
      try {
        await fetch("/api/update_environment_variables", {
          body: JSON.stringify({ changes }),
          method: "POST",
        });
      } catch (e) {
        return await logAndHandleFetchError(ctx, e);
      }
    },
    notice: deployment.deploymentNotice ?? "",
  };
}

export async function envSet(
  ctx: Context,
  backend: EnvVarBackend,
  originalName: string | undefined,
  originalValue: string | undefined,
  options?: {
    fromFile?: string;
    force?: boolean;
    secret?: boolean;
  },
) {
  const { fromFile, force = false } = options ?? {};
  if (originalName) {
    let name = originalName,
      value: string;
    const parsed = await allowEqualsSyntax(ctx, originalName, originalValue);
    if (parsed) {
      [name, value] = parsed;
    } else if (fromFile) {
      value = await getFileContents(ctx, fromFile);
    } else if (!process.stdin.isTTY) {
      value = await getStdIn(ctx);
    } else {
      value = await promptSecret(ctx, {
        message: `Enter value for ${name}:`,
      });
    }
    await backend.update([{ name, value }]);
    if (options?.secret) {
      const formatted = /\s/.test(value) ? `"${value}"` : value;
      logFinishedStep(
        `Successfully set ${chalkStderr.bold(name)} to ${chalkStderr.bold(formatted)}${backend.notice}`,
      );
    } else {
      logFinishedStep(
        `Successfully set ${chalkStderr.bold(name)}${backend.notice}`,
      );
    }
    return true;
  }
  let content: string, source: string;
  if (fromFile) {
    content = await getFileContents(ctx, fromFile);
    source = fromFile;
  } else if (!process.stdin.isTTY) {
    content = await getStdIn(ctx);
    source = "stdin";
  } else {
    return false;
  }
  await envSetFromContent(ctx, backend, {
    content,
    source,
    force,
  });
  return true;
}

export async function envGet(
  ctx: Context,
  backend: EnvVarBackend,
  name: string,
) {
  const envVar = await backend.get(name);
  if (envVar === null) {
    logFailure(`Environment variable "${name}" not found${backend.notice}`);
    return;
  }
  logOutput(`${envVar.value}`);
}

export async function envRemove(
  ctx: Context,
  backend: EnvVarBackend,
  name: string,
) {
  await backend.update([{ name, value: null }]);
  logFinishedStep(
    `Successfully unset ${chalkStderr.bold(name)}${backend.notice}`,
  );
}

export async function envList(
  ctx: Context,
  backend: EnvVarBackend,
  options?: { namesOnly?: boolean; includeValues?: boolean },
) {
  const envs = await backend.list();
  if (envs.length === 0) {
    logMessage(`No environment variables set${backend.notice}`);
    return;
  }
  // Agents tend to run `env list` to check what is set, which copies every
  // secret into their context. They get names unless they ask for values.
  const agent =
    options?.namesOnly || options?.includeValues ? null : await detectAiAgent();
  const namesOnly = options?.namesOnly || agent !== null;
  for (const { name, value } of envs) {
    if (namesOnly) {
      logOutput(name);
      continue;
    }
    const { formatted, warning } = formatEnvValueForDotfile(value);
    if (warning) {
      logMessage(`Warning (${name}): ${warning}`);
    }
    logOutput(`${name}=${formatted}`);
  }
  if (agent !== null) {
    logMessage(
      `Listed names only because an AI agent (${agent}) appears to be running this command. Pass --include-values to print the values too.`,
    );
  }
}

async function getFileContents(
  ctx: Context,
  filePath: string,
): Promise<string> {
  if (!ctx.fs.exists(filePath)) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `error: file not found: ${filePath}`,
    });
  }
  return ctx.fs.readUtf8File(filePath);
}

async function getStdIn(ctx: Context): Promise<string> {
  try {
    return await readFromStdin();
  } catch (error) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `error: failed to read from stdin: ${error instanceof Error ? error.message : String(error)}`,
    });
  }
}

async function envSetFromContent(
  ctx: Context,
  backend: EnvVarBackend,
  options: {
    content: string;
    source: string;
    force: boolean;
  },
) {
  const { content, source, force } = options;
  const parsedEnv = dotenv.parse(content);

  // Filter out CLI-managed environment variables
  const envVars = Object.entries(parsedEnv);
  const filteredVars: string[] = [];

  const envVarsToSet: [string, string][] = [];
  const managedVars = new Set<string>([
    CONVEX_DEPLOY_KEY_ENV_VAR_NAME,
    CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME,
    CONVEX_DEPLOYMENT_ENV_VAR_NAME,
    CONVEX_SELF_HOSTED_URL_VAR_NAME,
    CONVEX_SELF_HOSTED_ADMIN_KEY_VAR_NAME,
    ...EXPECTED_CONVEX_URL_NAMES,
    ...EXPECTED_SITE_URL_NAMES,
  ]);
  for (const [name, value] of envVars) {
    if (managedVars.has(name)) {
      filteredVars.push(name);
    } else {
      envVarsToSet.push([name, value]);
    }
  }

  if (filteredVars.length > 0) {
    const varNames = filteredVars.map((n) => chalkStderr.bold(n));
    const formattedNames = formatList(varNames);
    logMessage(
      `Skipping ${filteredVars.length} CLI-managed environment variable${filteredVars.length === 1 ? "" : "s"}: ${formattedNames}`,
    );
  }

  if (envVarsToSet.length === 0) {
    if (envVars.length === 0) {
      logMessage(`No environment variables found in ${source}.`);
    }
    return;
  }

  // Fetch existing environment variables
  const existingEnvVars = await backend.list();

  const existingEnvMap = new Map(
    existingEnvVars.map((env) => [env.name, env.value]),
  );

  // Categorize the environment variables
  const newVars: [string, string][] = [];
  const updatedVars: [string, string][] = [];
  const unchangedVars: [string, string][] = [];
  const conflicts: { name: string; existing: string; new: string }[] = [];

  for (const [name, value] of envVarsToSet) {
    const existingValue = existingEnvMap.get(name);
    if (existingValue === undefined) {
      newVars.push([name, value]);
    } else if (existingValue === value) {
      unchangedVars.push([name, value]);
    } else if (force) {
      updatedVars.push([name, value]);
    } else {
      conflicts.push({ name, existing: existingValue, new: value });
    }
  }

  // Check for conflicts if not replacing
  if (conflicts.length > 0) {
    const varNames = conflicts.map((c) => chalkStderr.bold(c.name));
    const formattedNames = formatList(varNames);
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        `error: environment variable${conflicts.length === 1 ? "" : "s"} ${formattedNames} already exist${conflicts.length === 1 ? "s" : ""} with different value${conflicts.length === 1 ? "" : "s"}.\n\n` +
        `Use ${chalkStderr.bold("--force")} to overwrite existing values.`,
    });
  }

  // Build the changes: only new vars when not replacing, new + updated when replacing
  const varsToUpdate = force ? [...newVars, ...updatedVars] : newVars;
  const changes: EnvVarChange[] = varsToUpdate.map(([name, value]) => ({
    name,
    value,
  }));

  if (changes.length > 0) {
    await backend.update(changes);
  }

  const newCount = newVars.length;
  const updatedCount = updatedVars.length;
  const unchangedCount = unchangedVars.length;

  const parts = [];
  if (newCount > 0) parts.push(`${newCount} new`);
  if (updatedCount > 0) parts.push(`${updatedCount} updated`);
  if (unchangedCount > 0) parts.push(`${unchangedCount} unchanged`);

  const totalProcessed = newCount + updatedCount + unchangedCount;
  if (changes.length === 0) {
    logMessage(
      `All ${totalProcessed} environment variable${totalProcessed === 1 ? "" : "s"} from ${chalkStderr.bold(source)} already set${backend.notice}`,
    );
  } else {
    logFinishedStep(
      `Successfully set ${changes.length} environment variable${changes.length === 1 ? "" : "s"} from ${chalkStderr.bold(source)} (${parts.join(", ")})${backend.notice}`,
    );
  }
}

async function allowEqualsSyntax(
  ctx: Context,
  name: string,
  value: string | undefined,
): Promise<[string, string] | null> {
  if (/^[a-zA-Z][a-zA-Z0-9_]*=/.test(name)) {
    const [n, ...values] = name.split("=");
    if (value === undefined) {
      return [n, values.join("=")];
    } else {
      await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `When setting an environment variable, you can either set a value with 'NAME=value', or with NAME value, but not both. Are you missing quotes around the CLI argument? Try: \n  npx convex env set '${name} ${value}'`,
      });
    }
  }
  if (value === undefined) return null;
  return [name, value];
}
