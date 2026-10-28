import { paths as PlatformDeploymentPaths } from "@convex-dev/platform/deploymentApi";
import { paths as PlatformManagementPaths } from "@convex-dev/platform/managementApi";
import { chalkStderr } from "chalk";
import os from "os";
import path from "path";

import { ProjectConfig } from "../config.js";

import { spawn } from "child_process";
import { InvalidArgumentError } from "commander";
import fetchRetryFactory, { RequestInitRetryParams } from "fetch-retry";
import { Context, ErrorType } from "../../../bundler/context.js";
import {
  failExistingSpinner,
  logError,
  logMessage,
  logWarning,
} from "../../../bundler/log.js";
import { version } from "../../version.js";
import type { CloudDeploymentType, Project } from "../api.js";
import { promptOptions, promptSearch, promptYesNo } from "./prompts.js";
import {
  bigBrainEnableFeatureMetadata,
  projectHasExistingCloudDev,
} from "../localDeployment/bigBrain.js";
import type {
  paths as CliManagementPaths,
  RegionName,
  TeamResponse,
} from "../../generatedApi.js";
import createClient from "openapi-fetch";

const retryingFetch = fetchRetryFactory(fetch);

export const productionProvisionHost = "https://api.convex.dev";
export const provisionHost =
  process.env.CONVEX_PROVISION_HOST || productionProvisionHost;
export const BIG_BRAIN_URL = `${provisionHost}/api/`;
const PLATFORM_MANAGEMENT_API_URL = `${provisionHost}/v1/`;
export const ENV_VAR_FILE_PATH = ".env.local";
export const CONVEX_DEPLOY_KEY_ENV_VAR_NAME = "CONVEX_DEPLOY_KEY";
// Alias for CONVEX_DEPLOY_KEY. Accepted anywhere CONVEX_DEPLOY_KEY is.
export const CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME = "CONVEX_DEPLOYMENT_TOKEN";
export const CONVEX_DEPLOYMENT_ENV_VAR_NAME = "CONVEX_DEPLOYMENT";
export const CONVEX_SELF_HOSTED_URL_VAR_NAME = "CONVEX_SELF_HOSTED_URL";
export const CONVEX_SELF_HOSTED_ADMIN_KEY_VAR_NAME =
  "CONVEX_SELF_HOSTED_ADMIN_KEY";
const MAX_RETRIES = 6;
// After 3 retries, log a progress message that we're retrying the request
const RETRY_LOG_THRESHOLD = 3;

/**
 * Processes the CONVEX_DEPLOY_KEY value to handle special sentinel values.
 *
 * - If the value is `<ignore_deploy_key>`, treats it as if the env var isn't set (returns undefined)
 * - If the value matches `<missing_deploy_key:$STRING>`, crashes with the message in $STRING
 * - Otherwise returns the value as-is
 *
 * @param ctx Context for crashing if needed
 * @param deployKey The raw deploy key value from environment or config
 * @returns The processed deploy key value or undefined
 */
export async function processDeployKeyValue(
  ctx: Context,
  deployKey: string | undefined,
): Promise<string | undefined> {
  if (deployKey === undefined) {
    return undefined;
  }

  // Check for <ignore_deploy_key> sentinel
  if (deployKey === "<ignore_deploy_key>") {
    return undefined;
  }

  // Check for <missing_deploy_key:$STRING> sentinel
  const missingKeyPattern = /^<missing_deploy_key:(.+)>$/;
  const match = deployKey.match(missingKeyPattern);
  if (match) {
    const errorMessage = match[1];
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: errorMessage,
    });
  }

  return deployKey;
}

/**
 * Reads the deploy key from environment variables, accepting either
 * CONVEX_DEPLOY_KEY or its alias CONVEX_DEPLOYMENT_TOKEN. CONVEX_DEPLOY_KEY
 * takes precedence when both are set.
 */
export function readDeployKeyFromEnv(
  getEnv: (name: string) => string | undefined | null,
): string | undefined {
  const fromDeployKey = getEnv(CONVEX_DEPLOY_KEY_ENV_VAR_NAME);
  const fromToken = getEnv(CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME);
  return fromDeployKey || fromToken || undefined;
}

// The prefix the backend's `require_operation` checks use when an identity
// lacks a permission. The full message names the missing permission, e.g.
// `You do not have permission to perform this operation (deployment:data:view).`
const PERMISSION_DENIED_MESSAGE_PREFIX =
  "You do not have permission to perform this operation";

function deployKeyPermissionAdvice(): string | null {
  // Name whichever env var is actually set. `CONVEX_DEPLOY_KEY` takes
  // precedence over `CONVEX_DEPLOYMENT_TOKEN` when both are present.
  const envVarName = process.env[CONVEX_DEPLOY_KEY_ENV_VAR_NAME]
    ? CONVEX_DEPLOY_KEY_ENV_VAR_NAME
    : process.env[CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME]
      ? CONVEX_DEPLOYMENT_TOKEN_ENV_VAR_NAME
      : null;
  if (envVarName === null) {
    return null;
  }
  return `This is determined by the permissions granted to ${envVarName}.`;
}

/**
 * Detects a server-side authorization failure (a `require_operation` error,
 * surfaced both as an HTTP 403 and as an error from a system UDF query) and
 * produces a clean, single-line message naming the missing permission. When a
 * deploy key is in use, appends advice for configuring its permissions.
 *
 * Returns `null` if `errorMessage` is not a permission error, so callers can
 * fall back to their normal error handling.
 */
export function permissionDeniedErrorMessage(
  errorMessage: string,
): string | null {
  const index = errorMessage.indexOf(PERMISSION_DENIED_MESSAGE_PREFIX);
  if (index === -1) {
    return null;
  }
  // Drop any wrapping context (e.g. the `[CONVEX Q(...)]` prefix) and stack
  // trace, keeping just the permission sentence.
  let msg = errorMessage.slice(index).split("\n")[0].trim();
  const advice = deployKeyPermissionAdvice();
  if (advice) {
    msg = `${msg}\n${advice}`;
  }
  return msg;
}

export function parsePositiveInteger(value: string) {
  const parsedValue = parseInteger(value);
  if (parsedValue <= 0) {
    // eslint-disable-next-line no-restricted-syntax
    throw new InvalidArgumentError("Not a positive number.");
  }
  return parsedValue;
}

export function parseInteger(value: string) {
  const parsedValue = +value;
  if (isNaN(parsedValue)) {
    // eslint-disable-next-line no-restricted-syntax
    throw new InvalidArgumentError("Not a number.");
  }
  return parsedValue;
}

export type ErrorData = {
  code: string;
  message: string;
};

/**
 * Error thrown on non-2XX reponse codes to make most `fetch()` error handling
 * follow a single code path.
 */
export class ThrowingFetchError extends Error {
  response: Response;
  serverErrorData?: ErrorData;

  constructor(
    msg: string,
    {
      code,
      message,
      response,
    }: { cause?: Error; code?: string; message?: string; response: Response },
  ) {
    if (code !== undefined && message !== undefined) {
      super(`${msg}: ${code}: ${message}`);
      this.serverErrorData = { code, message };
    } else {
      super(msg);
    }

    Object.setPrototypeOf(this, ThrowingFetchError.prototype);

    this.response = response;
  }

  public static async fromResponse(
    response: Response,
    msg?: string,
  ): Promise<ThrowingFetchError> {
    msg = `${msg ? `${msg} ` : ""}${response.status} ${response.statusText}`;
    let code, message;
    try {
      ({ code, message } = await response.json());
    } catch {
      // Do nothing because the non-2XX response code is the primary error here.
    }
    return new ThrowingFetchError(msg, { code, message, response });
  }

  async handle(ctx: Context): Promise<never> {
    let error_type: ErrorType = "transient";
    await checkFetchErrorForDeprecation(ctx, this.response);

    let msg = this.message;

    if (this.response.status === 400) {
      error_type = "invalid filesystem or env vars";
    } else if (this.response.status === 401) {
      error_type = "fatal";
      msg = `${msg}\nAuthenticate with \`npx convex dev\``;
    } else if (this.response.status === 403) {
      error_type = "fatal";
      // Surface the missing permission cleanly (e.g. the server includes the
      // required action like `deployment:data:view`) instead of the noisy
      // "Error fetching ..." wrapper.
      msg = this.serverErrorData?.message ?? msg;
      const advice = deployKeyPermissionAdvice();
      if (advice) {
        msg = `${msg}\n${advice}`;
      }
    } else if (this.response.status === 404) {
      error_type = "fatal";
      msg = `${msg}: ${this.response.url}`;
    }

    return await ctx.crash({
      exitCode: 1,
      errorType: error_type,
      errForSentry: this,
      printedMessage: chalkStderr.red(msg.trim()),
    });
  }
}

/**
 * Thin wrapper around `fetch()` which throws a FetchDataError on non-2XX
 * responses which includes error code and message from the response JSON.
 * (Axios-style)
 *
 * It also accepts retry options from fetch-retry.
 */
export async function throwingFetch(
  resource: RequestInfo | URL,
  options: (RequestInit & RequestInitRetryParams<typeof fetch>) | undefined,
): Promise<Response> {
  const response = await retryingFetch(resource, options);
  if (!response.ok) {
    // This error must always be handled manually.
    // eslint-disable-next-line no-restricted-syntax
    throw await ThrowingFetchError.fromResponse(
      response,
      `Error fetching ${options?.method ? options.method + " " : ""} ${
        typeof resource === "string"
          ? resource
          : "url" in resource
            ? resource.url
            : resource.toString()
      }`,
    );
  }
  return response;
}

/**
 * Handle an error a fetch error or non-2xx response.
 */
export async function logAndHandleFetchError(
  ctx: Context,
  err: unknown,
): Promise<never> {
  // Fail the spinner so the stderr lines appear
  failExistingSpinner();

  if (err instanceof ThrowingFetchError) {
    return await err.handle(ctx);
  } else {
    return await ctx.crash({
      exitCode: 1,
      errorType: "transient",
      errForSentry: err,
      printedMessage: chalkStderr.red(err),
    });
  }
}

function logDeprecationWarning(ctx: Context, deprecationMessage: string) {
  if (ctx.deprecationMessagePrinted) {
    return;
  }
  ctx.deprecationMessagePrinted = true;
  logWarning(chalkStderr.yellow(deprecationMessage));
}

async function checkFetchErrorForDeprecation(ctx: Context, resp: Response) {
  const headers = resp.headers;
  if (headers) {
    const deprecationState = headers.get("x-convex-deprecation-state");
    const deprecationMessage = headers.get("x-convex-deprecation-message");
    switch (deprecationState) {
      case null:
        break;
      case "Deprecated":
        // This version is deprecated. Print a warning and crash.

        // Gotcha:
        // 1. Don't use `logDeprecationWarning` because we should always print
        // why this we crashed (even if we printed a warning earlier).
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: chalkStderr.red(deprecationMessage),
        });
      default:
        // The error included a deprecation warning. Print, but handle the
        // error normally (it was for another reason).
        logDeprecationWarning(
          ctx,
          deprecationMessage || "(no deprecation message included)",
        );
        break;
    }
  }
}

/// Call this method after a successful API response to conditionally print the
/// "please upgrade" message.
export function deprecationCheckWarning(ctx: Context, resp: Response) {
  const headers = resp.headers;
  if (headers) {
    const deprecationState = headers.get("x-convex-deprecation-state");
    const deprecationMessage = headers.get("x-convex-deprecation-message");
    switch (deprecationState) {
      case null:
        break;
      case "Deprecated":
        // This should never happen because such states are errors, not warnings.
        // eslint-disable-next-line no-restricted-syntax
        throw new Error(
          "Called deprecationCheckWarning on a fatal error. This is a bug.",
        );
      default:
        logDeprecationWarning(
          ctx,
          deprecationMessage || "(no deprecation message included)",
        );
        break;
    }
  }
}

export async function hasTeam(ctx: Context, teamSlug: string) {
  const teams = (await typedBigBrainClient(ctx).GET("/teams")).data!;
  return teams.some((team) => team.slug === teamSlug);
}

export async function validateOrSelectTeam(
  ctx: Context,
  teamSlug: string | undefined,
  promptMessage: string,
): Promise<{ team: TeamResponse; chosen: boolean }> {
  const teams = (await typedBigBrainClient(ctx).GET("/teams")).data!;
  if (teams.length === 0) {
    await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      errForSentry: "No teams found",
      printedMessage: chalkStderr.red("Error: No teams found"),
    });
  }
  if (!teamSlug) {
    // Prompt the user to select if they belong to more than one team.
    switch (teams.length) {
      case 1:
        return { team: teams[0], chosen: false };
      default: {
        const teamSlug = await promptSearch(ctx, {
          message: promptMessage,
          choices: teams.map((team) => ({
            name: `${team.name} (${team.slug})`,
            value: team.slug,
          })),
        });
        const team = teams.find((team) => team.slug === teamSlug);
        if (!team) {
          return await ctx.crash({
            exitCode: 1,
            errorType: "fatal",
            printedMessage: `Error: Failed to select team`,
          });
        }
        return { team, chosen: true };
      }
    }
  } else {
    // Validate the chosen team.
    const team = teams.find((team) => team.slug === teamSlug);
    if (!team) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Error: Team ${teamSlug} not found, fix the --team option or remove it`,
      });
    }
    return { team, chosen: false };
  }
}

export async function selectDevDeploymentType(
  ctx: Context,
  {
    chosenConfiguration,
    newOrExisting,
    teamSlug,
    projectSlug,
    userHasChosenSomethingInteractively,
    // from `--configure --dev-deployment local|cloud`
    devDeploymentFromFlag,
  }:
    | {
        chosenConfiguration: "new" | "existing" | "ask" | null;
        newOrExisting: "existing";
        teamSlug: string;
        projectSlug: string;
        userHasChosenSomethingInteractively: boolean;
        devDeploymentFromFlag: "cloud" | "local" | undefined;
      }
    | {
        chosenConfiguration: "new" | "existing" | "ask" | null;
        newOrExisting: "new";
        teamSlug: string;
        // For new projects we don't know the project slug yet.
        projectSlug: undefined;
        userHasChosenSomethingInteractively: boolean;
        devDeploymentFromFlag: "cloud" | "local" | undefined;
      },
): Promise<{ devDeployment: "cloud" | "local" }> {
  if (devDeploymentFromFlag) return { devDeployment: devDeploymentFromFlag };

  if (newOrExisting === "existing" && chosenConfiguration === null) {
    // Don't suggest local dev if developer already has a cloud deployment.
    if (await projectHasExistingCloudDev(ctx, { projectSlug, teamSlug })) {
      // TODO Expand rollout to offer local dev in this case. ENG-8307
      return { devDeployment: "cloud" };
    }
  }

  // To avoid breaking previously non-interactive flows, don't prompt if enough
  // flags were specified for configure not to already have needed input.
  if (chosenConfiguration !== "ask" && !userHasChosenSomethingInteractively) {
    return { devDeployment: "cloud" };
  }

  // For creating a first project (no projects exist) or joining a first project
  // (one project exists), always use cloud since it's a smoother experience.
  const isFirstProject =
    (await bigBrainEnableFeatureMetadata(ctx)).totalProjects.kind !==
    "multiple";
  if (isFirstProject) {
    return { devDeployment: "cloud" };
  }

  // For now default is always cloud.
  const devDeployment: "cloud" | "local" = await promptOptions(ctx, {
    message:
      "Use cloud or local dev deployment? For more see https://docs.convex.dev/cli/local-deployments",
    default: "cloud",
    choices: [
      { name: "cloud deployment", value: "cloud" },
      { name: "local deployment (BETA)", value: "local" },
    ],
  });
  return { devDeployment };
}

export async function logNoDefaultRegionMessage(teamSlug: string) {
  const { teamDashboardUrl } = await import("../dashboard.js");
  const noDefaultRegionMessage = chalkStderr.gray(
    `Tip: you can configure a default region for your team at ${chalkStderr.underline(`${teamDashboardUrl(teamSlug)}/settings`)}`,
  );
  logMessage(noDefaultRegionMessage);
}

export async function selectRegionOrUseDefault(
  ctx: Context,
  selectedTeam: TeamResponse,
  deploymentType: CloudDeploymentType,
) {
  if (!process.stdin.isTTY) {
    // Use the team default in non-interactive terminals
    if (!selectedTeam.defaultRegion) {
      await logNoDefaultRegionMessage(selectedTeam.slug);
    }
    return selectedTeam.defaultRegion ?? null;
  }
  const selectedRegionName =
    selectedTeam.defaultRegion ??
    (await selectRegion(ctx, selectedTeam.id, deploymentType));
  if (!selectedTeam.defaultRegion) {
    // Log after the user chooses a region
    await logNoDefaultRegionMessage(selectedTeam.slug);
  }
  return selectedRegionName;
}

// Mirrors the order of `REGION_PRESENTATION` in
// `npm-packages/dashboard/src/lib/regions.ts`, so both region pickers list
// regions in the same order.
const REGION_ORDER: string[] = [
  "aws-us-east-1",
  "aws-eu-west-1",
  "aws-ca-central-1",
  "aws-ap-southeast-2",
];

// Regions missing from `REGION_ORDER` (e.g. newly launched ones) go last, in
// server order, since `Array.prototype.sort` is stable.
function regionSortIndex(regionName: string): number {
  const index = REGION_ORDER.indexOf(regionName);
  return index === -1 ? REGION_ORDER.length : index;
}

export async function selectRegion(
  ctx: Context,
  teamId: number,
  deploymentType: CloudDeploymentType,
): Promise<RegionName> {
  const regionsResponse = (
    await typedPlatformClient(ctx).GET(
      "/teams/{team_id}/list_deployment_regions",
      {
        params: {
          path: { team_id: `${teamId}` },
        },
      },
    )
  ).data!;
  const choices = regionsResponse.items
    .filter((item) => Boolean(item.available))
    .map((item) => ({
      name: item.displayName,
      value: item.name,
    }))
    .sort((a, b) => regionSortIndex(a.value) - regionSortIndex(b.value));
  return await promptOptions(ctx, {
    message: `Where should this ${deploymentType} deployment run?`,
    suffix: `\n${chalkStderr.gray(
      "See https://www.convex.dev/pricing for pricing",
    )}`,
    choices,
  });
}

export async function hasProject(
  ctx: Context,
  teamSlug: string,
  projectSlug: string,
) {
  try {
    await typedPlatformClient(ctx, { throw: true }).GET(
      "/teams/{team_id_or_slug}/projects/{project_slug}",
      {
        params: {
          path: {
            team_id_or_slug: teamSlug,
            project_slug: projectSlug,
          },
        },
      },
    );
    return true;
  } catch {
    return false;
  }
}

export async function hasProjects(ctx: Context) {
  return !!(await bigBrainAPI({ ctx, method: "GET", path: `has_projects` }));
}

export async function validateOrSelectProject(
  ctx: Context,
  projectSlug: string | undefined,
  teamId: number,
  singleProjectPrompt: string,
  multiProjectPrompt: string,
): Promise<string | null> {
  const projects: Project[] = [];
  const client = typedPlatformClient(ctx);
  let cursor: string | undefined;
  do {
    const { items, pagination } = (
      await client.GET("/teams/{team_id}/projects", {
        params: {
          path: { team_id: teamId },
          query: cursor === undefined ? {} : { cursor },
        },
      })
    ).data!;
    projects.push(...items);
    cursor = pagination.hasMore
      ? (pagination.nextCursor ?? undefined)
      : undefined;
  } while (cursor !== undefined);
  if (projects.length === 0) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `No existing projects! Run this command again and choose "create a new project."`,
    });
  }
  if (!projectSlug) {
    // Prompt the user to select project.
    switch (projects.length) {
      case 1: {
        const project = projects[0];
        const confirmed = await promptYesNo(ctx, {
          message: `${singleProjectPrompt} ${project.name} (${project.slug})?`,
        });

        if (!confirmed) {
          return null;
        }
        return projects[0].slug;
      }
      default:
        return await promptSearch(ctx, {
          message: multiProjectPrompt,
          choices: projects.map((project: Project) => ({
            name: `${project.name} (${project.slug})`,
            value: project.slug,
          })),
        });
    }
  } else {
    // Validate the chosen project.
    if (!projects.find((project) => project.slug === projectSlug)) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `Error: Project ${projectSlug} not found, fix the --project option or remove it`,
      });
    }
    return projectSlug;
  }
}

/**
 * @param ctx
 * @returns a Record of dependency name to dependency version for dependencies
 * and devDependencies
 */
export async function loadPackageJson(
  ctx: Context,
  includePeerDeps = false,
): Promise<Record<string, string>> {
  let packageJson;
  try {
    packageJson = ctx.fs.readUtf8File("package.json");
  } catch (err) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Unable to read your package.json: ${
        err as any
      }. Make sure you're running this command from the root directory of a Convex app that contains the package.json`,
    });
  }
  let obj;
  try {
    obj = JSON.parse(packageJson);
  } catch (err) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      errForSentry: err,
      printedMessage: `Unable to parse package.json: ${err as any}`,
    });
  }
  if (typeof obj !== "object") {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: "Expected to parse an object from package.json",
    });
  }
  const packages = {
    ...(includePeerDeps ? (obj.peerDependencies ?? {}) : {}),
    ...(obj.dependencies ?? {}),
    ...(obj.devDependencies ?? {}),
  };
  return packages;
}

export async function ensureHasConvexDependency(ctx: Context, cmd: string) {
  const packages = await loadPackageJson(ctx, true);
  const hasConvexDependency = "convex" in packages;
  if (!hasConvexDependency) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `In order to ${cmd}, add \`convex\` to your package.json dependencies.`,
    });
  }
}

/** Return a new array with elements of the passed in array sorted by a key lambda */
export const sorted = <T>(arr: T[], key: (el: T) => any): T[] => {
  const newArr = [...arr];
  const cmp = (a: T, b: T) => {
    if (key(a) < key(b)) return -1;
    if (key(a) > key(b)) return 1;
    return 0;
  };
  return newArr.sort(cmp);
};

export function functionsDir(
  configPath: string,
  projectConfig: ProjectConfig,
): string {
  return path.join(path.dirname(configPath), projectConfig.functions);
}

function convexName() {
  // Use a different directory for config files generated for tests
  if (process.env.CONVEX_PROVISION_HOST) {
    const port = process.env.CONVEX_PROVISION_HOST.split(":")[2];
    if (port === undefined || port === "8050") {
      return `convex-test`;
    } else {
      return `convex-test-${port}`;
    }
  }
  return "convex";
}

export function rootDirectory(): string {
  return path.join(os.homedir(), `.${convexName()}`);
}

export function cacheDir() {
  const name = convexName();
  const platform = process.platform;
  if (platform === "win32") {
    // On Windows, `LOCALAPPDATA` is usually set, but fall back to
    // `USERPROFILE` if not, and fall back to homedir if all else fails.
    if (process.env.LOCALAPPDATA) {
      return path.join(process.env.LOCALAPPDATA, name);
    }
    if (process.env.USERPROFILE) {
      return path.join(process.env.USERPROFILE, "AppData", "Local", name);
    }
    return path.join(os.homedir(), "AppData", "Local", name);
  }
  return path.join(os.homedir(), ".cache", name);
}

/**
 * Fetch with appropriate headers for the Convex Management API.
 *
 * This fetch() also has retries and throws if the response is not ok.
 */
export function bigBrainFetch(ctx: Context): typeof fetch {
  const authHeader = ctx.bigBrainAuth()?.header;
  const bigBrainHeaders: Record<string, string> = authHeader
    ? {
        Authorization: authHeader,
        "Convex-Client": `npm-cli-${version}`,
      }
    : {
        "Convex-Client": `npm-cli-${version}`,
      };
  return (resource: RequestInfo | URL, options: RequestInit | undefined) => {
    const { headers: optionsHeaders, ...rest } = options || {};
    // Use the options headers if they exist. Otherwise, use the request headers
    const headers = {
      ...bigBrainHeaders,
      ...(optionsHeaders ??
        (resource instanceof Request
          ? Object.fromEntries(resource.headers.entries())
          : {})),
    };
    const opts = {
      retries: MAX_RETRIES,
      retryDelay,
      headers,
      ...rest,
    };

    return throwingFetch(resource, opts);
  };
}

export async function bigBrainAPI<T = any>({
  ctx,
  method,
  path,
  data,
}: {
  ctx: Context;
  method: "GET" | "POST" | "HEAD";
  path: string;
  data?: any;
}): Promise<T> {
  const dataString =
    data === undefined
      ? undefined
      : typeof data === "string"
        ? data
        : JSON.stringify(data);
  try {
    return await bigBrainAPIMaybeThrows({
      ctx,
      method,
      path,
      data: dataString,
    });
  } catch (err: unknown) {
    return await logAndHandleFetchError(ctx, err);
  }
}

/**
 * Typed API client with a fetch() implemention that includes retries and crashes on errors.
 * It is always safe to call `.data!` on the response: any error would throw or crash.
 *
 * Pass { throw: true } to throw ThrowingFetchErrors instead of exiting the process.
 */
function typedApiClientFactory<T>(
  baseUrl: string,
  fetchBuilder: (ctx: Context) => typeof fetch,
) {
  return (ctx: Context, options: { throw?: boolean } = {}) => {
    type Paths = T extends CliManagementPaths
      ? CliManagementPaths
      : T extends PlatformManagementPaths
        ? PlatformManagementPaths
        : T extends PlatformDeploymentPaths
          ? PlatformDeploymentPaths
          : never;
    const client = createClient<Paths>({
      baseUrl,
      fetch: fetchBuilder(ctx),
    });

    // Wrap the client with error handling - go back to proxy since middleware doesn't catch parsing errors
    return new Proxy(client, {
      get(target, prop) {
        const originalMethod = target[prop as keyof typeof target];

        if (
          prop === "GET" ||
          prop === "POST" ||
          prop === "HEAD" ||
          prop === "OPTIONS" ||
          prop === "PUT" ||
          prop === "DELETE" ||
          prop === "PATCH" ||
          prop === "TRACE"
        ) {
          return async (...args: any[]) => {
            try {
              return await (originalMethod as Function).apply(target, args);
            } catch (err: unknown) {
              if (options.throw) {
                // eslint-disable-next-line no-restricted-syntax
                throw err;
              }
              return await logAndHandleFetchError(ctx, err);
            }
          };
        }

        return originalMethod;
      },
    });
  };
}

export const typedBigBrainClient = typedApiClientFactory<CliManagementPaths>(
  BIG_BRAIN_URL,
  bigBrainFetch,
);
export const typedPlatformClient =
  typedApiClientFactory<PlatformManagementPaths>(
    PLATFORM_MANAGEMENT_API_URL,
    bigBrainFetch,
  );

export function typedDeploymentClient(
  ctx: Context,
  args: { deploymentUrl: string; adminKey: string },
  options: { throw?: boolean } = {},
) {
  return typedApiClientFactory<PlatformDeploymentPaths>(
    `${args.deploymentUrl}/api/v1`,
    (ctx) =>
      deploymentFetch(ctx, {
        deploymentUrl: args.deploymentUrl,
        adminKey: args.adminKey,
      }),
  )(ctx, options);
}

export async function bigBrainAPIMaybeThrows({
  ctx,
  method,
  path,
  data,
}: {
  ctx: Context;
  method: "GET" | "POST" | "HEAD";
  path: string;
  data?: any;
}): Promise<any> {
  const fetch = bigBrainFetch(ctx);
  const dataString =
    data === undefined
      ? method === "POST"
        ? JSON.stringify({})
        : undefined
      : typeof data === "string"
        ? data
        : JSON.stringify(data);
  const res = await fetch(new URL(path, BIG_BRAIN_URL), {
    method,
    ...(dataString ? { body: dataString } : {}),
    headers:
      method === "POST"
        ? {
            "Content-Type": "application/json",
          }
        : {},
  });
  deprecationCheckWarning(ctx, res);
  if (res.status === 200) {
    return await res.json();
  }
}

/**
 * Polls an arbitrary function until a condition is met.
 *
 * @param fetch Function performing a fetch, returning resulting data.
 * @param condition This function will terminate polling when it returns `true`.
 * @param waitMs How long to wait in between fetches.
 * @returns The resulting data from `fetch`.
 */
export const poll = async function <Result>(
  fetch: () => Promise<Result>,
  condition: (data: Result) => boolean,
  waitMs = 1000,
) {
  let result = await fetch();
  while (!condition(result)) {
    await wait(waitMs);
    result = await fetch();
  }
  return result;
};

const wait = function (waitMs: number) {
  return new Promise((resolve) => {
    setTimeout(resolve, waitMs);
  });
};

export function waitForever() {
  // This never resolves
  return new Promise((_) => {
    // ignore
  });
}

// Returns a promise and a function that resolves the promise.
export function waitUntilCalled(): [Promise<unknown>, () => void] {
  let onCalled: (v: unknown) => void;
  const waitPromise = new Promise((resolve) => (onCalled = resolve));
  return [waitPromise, () => onCalled(null)];
}

const BYTE_UNITS = ["bytes", "KiB", "MiB", "GiB", "TiB", "PiB"];

/**
 * Format a byte count into a human-friendly string, rounded to one decimal
 * place (dropped when it would be ".0"), e.g. "4.1 MiB" or "128 MiB".
 */
export function formatSize(n: number): string {
  let value = n;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
    value /= 1024;
    unitIndex++;
  }
  // Rounding to one decimal place can itself reach the next unit's
  // boundary (e.g. 1023.999 KiB -> "1024.0 KiB" instead of "1.0 MiB").
  const roundedValue = Number(value.toFixed(1));
  if (roundedValue >= 1024 && unitIndex < BYTE_UNITS.length - 1) {
    value /= 1024;
    unitIndex++;
  }
  const formatted = value.toFixed(1).replace(/\.0$/, "");
  return `${formatted} ${BYTE_UNITS[unitIndex]}`;
}

/**
 * Read an integer override from the environment, falling back to
 * `defaultValue` when the variable is unset. The whole value must be a
 * base-10 integer: `parseInt` would read a typo like `100_000` or `100k` as
 * 100 and silently move the threshold, so anything else is rejected with a
 * warning instead.
 */
export function integerFromEnv(envVar: string, defaultValue: number): number {
  const envValue = process.env[envVar];
  if (envValue === undefined) {
    return defaultValue;
  }
  if (!/^[+-]?\d+$/.test(envValue.trim())) {
    logWarning(
      `Ignoring ${envVar}=${JSON.stringify(envValue)}: expected an integer, using ${defaultValue}`,
    );
    return defaultValue;
  }
  return Number(envValue);
}

export function formatDuration(ms: number): string {
  const twoDigits = (n: number, unit: string) =>
    `${n.toLocaleString("en-US", { maximumFractionDigits: 2 })}${unit}`;

  if (ms < 1e-3) {
    return twoDigits(ms * 1e9, "ns");
  }
  if (ms < 1) {
    return twoDigits(ms * 1e3, "µs");
  }
  if (ms < 1e3) {
    return twoDigits(ms, "ms");
  }
  const s = ms / 1e3;
  if (s < 60) {
    return twoDigits(ms / 1e3, "s");
  }
  return twoDigits(s / 60, "m");
}

export function getCurrentTimeString() {
  const now = new Date();
  const hours = String(now.getHours()).padStart(2, "0");
  const minutes = String(now.getMinutes()).padStart(2, "0");
  const seconds = String(now.getSeconds()).padStart(2, "0");
  return `${hours}:${minutes}:${seconds}`;
}

// We don't allow running commands in project subdirectories yet,
// but we can provide better errors if we look around.
export async function findParentConfigs(ctx: Context): Promise<{
  parentPackageJson: string;
  parentConvexJson?: string | undefined;
}> {
  const parentPackageJson = findUp(ctx, "package.json");
  if (!parentPackageJson) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage:
        "No package.json found. To create a new project using Convex, see https://docs.convex.dev/home#quickstarts",
    });
  }
  const candidateConvexJson =
    parentPackageJson &&
    path.join(path.dirname(parentPackageJson), "convex.json");
  const parentConvexJson =
    candidateConvexJson && ctx.fs.exists(candidateConvexJson)
      ? candidateConvexJson
      : undefined;
  return {
    parentPackageJson,
    parentConvexJson,
  };
}

/**
 * Finds a file in the current working directory or a parent.
 *
 * @returns The absolute path of the first file found or undefined.
 */
function findUp(ctx: Context, filename: string): string | undefined {
  let curDir = path.resolve(".");
  let parentDir = curDir;
  do {
    const candidate = path.join(curDir, filename);
    if (ctx.fs.exists(candidate)) {
      return candidate;
    }
    curDir = parentDir;
    parentDir = path.dirname(curDir);
  } while (parentDir !== curDir);
  return;
}

/**
 * Returns whether there's an existing project config. Throws
 * if this is not a valid directory for a project config.
 */
export async function isInExistingProject(ctx: Context) {
  const { parentPackageJson, parentConvexJson } = await findParentConfigs(ctx);
  if (parentPackageJson !== path.resolve("package.json")) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: "Run this command from the root directory of a project.",
    });
  }
  return !!parentConvexJson;
}

// `spawnAsync` is the async version of Node's `spawnSync` (and `spawn`).
//
// By default, this returns the produced `stdout` and `stderror` and
// an error if one was encountered (to mirror `spawnSync`).
//
// If `stdio` is set to `"inherit"`, pipes `stdout` and `stderror` (
// pausing the spinner if one is running) and rejects the promise
// on errors (to mirror `execFileSync`).
export function spawnAsync(
  ctx: Context,
  command: string,
  args: ReadonlyArray<string>,
): Promise<{
  stdout: string;
  stderr: string;
  status: null | number;
  error?: Error | undefined;
}>;
export function spawnAsync(
  ctx: Context,
  command: string,
  args: ReadonlyArray<string>,
  options: { stdio: "inherit"; shell?: boolean },
): Promise<void>;
export function spawnAsync(
  _ctx: Context,
  command: string,
  args: ReadonlyArray<string>,
  options?: { stdio: "inherit"; shell?: boolean },
) {
  return new Promise((resolve, reject) => {
    const child = spawn(command, args, { shell: options?.shell });
    let stdout = "";
    let stderr = "";

    const pipeOutput = options?.stdio === "inherit";

    if (pipeOutput) {
      child.stdout.on("data", (text) =>
        logMessage(text.toString("utf-8").trimEnd()),
      );
      child.stderr.on("data", (text) =>
        logError(text.toString("utf-8").trimEnd()),
      );
    } else {
      child.stdout.on("data", (data) => {
        stdout += data.toString("utf-8");
      });

      child.stderr.on("data", (data) => {
        stderr += data.toString("utf-8");
      });
    }

    const completionListener = (code: number | null) => {
      child.removeListener("error", errorListener);
      const result = pipeOutput
        ? { status: code }
        : { stdout, stderr, status: code };
      if (code !== 0) {
        const argumentString =
          args && args.length > 0 ? ` ${args.join(" ")}` : "";
        const error = new Error(
          `\`${command}${argumentString}\` exited with non-zero code: ${code}`,
        );
        if (pipeOutput) {
          reject({ ...result, error });
        } else {
          resolve({ ...result, error });
        }
      } else {
        resolve(result);
      }
    };

    const errorListener = (error: Error) => {
      child.removeListener("exit", completionListener);
      child.removeListener("close", completionListener);
      if (pipeOutput) {
        reject({ error, status: null });
      } else {
        resolve({ error, status: null });
      }
    };

    if (pipeOutput) {
      child.once("exit", completionListener);
    } else {
      child.once("close", completionListener);
    }
    child.once("error", errorListener);
  });
}

const IDEMPOTENT_METHODS = ["GET", "HEAD", "PUT", "DELETE", "OPTIONS", "TRACE"];

// immediate, 1s delay, 2s delay, 4s delay, etc.
export function retryDelay(
  attempt: number,
  _error: Error | null,
  _response: Response | null,
): number {
  const delay = attempt === 0 ? 1 : 2 ** (attempt - 1) * 1000;
  const randomSum = delay * 0.2 * Math.random();
  return delay + randomSum;
}

function deploymentFetchRetryOn(
  onError?: (err: any, attempt: number) => void,
  method?: string,
  maxRetries: number = MAX_RETRIES,
  shouldRetryOverride?: (
    error: Error | null,
    response: Response | null,
  ) => boolean,
) {
  const defaultShouldRetry = function (
    _attempt: number,
    error: Error | null,
    response: Response | null,
  ): { kind: "retry"; error: any } | { kind: "stop" } {
    // Retry on network errors.
    if (error !== null) {
      // TODO filter out all SSL errors
      // https://github.com/nodejs/node/blob/8a41d9b636be86350cd32847c3f89d327c4f6ff7/src/crypto/crypto_common.cc#L218-L245
      return { kind: "retry", error: error };
    }
    // Retry on 404s since these can sometimes happen with newly created
    // deployments for POSTs.
    if (response?.status === 404) {
      return {
        kind: "retry",
        error: `Received response with status ${response.status}`,
      };
    }

    // Whatever the error code it doesn't hurt to retry idempotent requests.
    if (
      response &&
      !response.ok &&
      method &&
      IDEMPOTENT_METHODS.includes(method.toUpperCase())
    ) {
      // ...but it's a bit annoying to wait for things we know won't succced
      if (
        [
          400, // Bad Request
          401, // Unauthorized
          402, // PaymentRequired
          403, // Forbidden
          405, // Method Not Allowed
          406, // Not Acceptable
          412, // Precondition Failed
          413, // Payload Too Large
          414, // URI Too Long
          415, // Unsupported Media Type
          416, // Range Not Satisfiable
        ].includes(response.status)
      ) {
        return {
          kind: "stop",
        };
      }
      return {
        kind: "retry",
        error: `Received response with status ${response.status}`,
      };
    }

    return { kind: "stop" };
  };

  const shouldRetry = shouldRetryOverride
    ? function (
        _attempt: number,
        error: Error | null,
        response: Response | null,
      ): { kind: "retry"; error: any } | { kind: "stop" } {
        return shouldRetryOverride(error, response)
          ? {
              kind: "retry",
              error:
                error ?? `Received response with status ${response?.status}`,
            }
          : { kind: "stop" };
      }
    : defaultShouldRetry;
  return function (
    attempt: number,
    error: Error | null,
    response: Response | null,
  ) {
    const result = shouldRetry(attempt, error, response);
    if (result.kind === "retry") {
      onError?.(result.error, attempt);
    }
    if (attempt >= maxRetries) {
      // Stop retrying if we've exhausted all retries, but do this after we've
      // called `onError` so that the caller can still log the error.
      return false;
    }
    return result.kind === "retry";
  };
}

/**
 * Unlike `deploymentFetch`, this does not add on any headers, so the caller
 * must supply any headers.
 */
export function bareDeploymentFetch(
  _ctx: Context,
  options: {
    deploymentUrl: string;
    onError?: (err: any) => void;
  },
): typeof throwingFetch {
  const { deploymentUrl, onError } = options;
  const onErrorWithAttempt = (err: any, attempt: number) => {
    onError?.(err);
    if (attempt >= RETRY_LOG_THRESHOLD) {
      logMessage(
        chalkStderr.gray(
          `Retrying request (attempt ${attempt}/${MAX_RETRIES})...`,
        ),
      );
    }
  };
  return (resource: RequestInfo | URL, options: RequestInit | undefined) => {
    const url =
      resource instanceof URL
        ? resource.pathname
        : typeof resource === "string"
          ? new URL(resource, deploymentUrl)
          : new URL(resource.url, deploymentUrl);
    const func = throwingFetch(url, {
      retryDelay,
      retryOn: deploymentFetchRetryOn(onErrorWithAttempt, options?.method),
      ...options,
    });
    return func;
  };
}

/**
 * This returns a `fetch` function that will fetch against `deploymentUrl`.
 *
 * It will also set the `Authorization` header, `Content-Type` header, and
 * the `Convex-Client` header if they are not set in the `fetch`.
 */
export function deploymentFetch(
  _ctx: Context,
  options: {
    deploymentUrl: string;
    adminKey: string;
    onError?: (err: any) => void;
    // Defaults to MAX_RETRIES. Lower this for best-effort requests (e.g.
    // advisory checks) where a slow retry loop against an unsupported or
    // unhealthy endpoint shouldn't hold up the surrounding command.
    maxRetries?: number | undefined;
    // Replaces the default retry policy (which is tuned for pushes: network
    // errors, 404s on fresh deployments, retries of idempotent methods).
    // Return true to retry the request. Backoff and retry logging are
    // shared either way.
    shouldRetry?: (error: Error | null, response: Response | null) => boolean;
  },
): typeof fetch {
  const {
    deploymentUrl,
    adminKey,
    onError,
    maxRetries = MAX_RETRIES,
    shouldRetry,
  } = options;
  const onErrorWithAttempt = (err: any, attempt: number) => {
    onError?.(err);
    if (attempt >= RETRY_LOG_THRESHOLD) {
      logMessage(
        chalkStderr.gray(
          `Retrying request (attempt ${attempt}/${maxRetries})...`,
        ),
      );
    }
  };
  return (resource: RequestInfo | URL, options: RequestInit | undefined) => {
    const url =
      resource instanceof URL
        ? resource.pathname
        : typeof resource === "string"
          ? new URL(resource, deploymentUrl)
          : new URL(resource.url, deploymentUrl);

    const headers = new Headers(options?.headers || {});
    if (!headers.has("Authorization")) {
      headers.set("Authorization", `Convex ${adminKey}`);
    }
    if (!headers.has("Content-Type")) {
      headers.set("Content-Type", "application/json");
    }
    if (!headers.has("Convex-Client")) {
      headers.set("Convex-Client", `npm-cli-${version}`);
    }
    const func = throwingFetch(url, {
      retryDelay,
      retryOn: deploymentFetchRetryOn(
        onErrorWithAttempt,
        options?.method,
        maxRetries,
        shouldRetry,
      ),
      ...options,
      headers,
    });
    return func;
  };
}

/**
 * Whether this is likely to be a WebContainer,
 * WebContainers can't complete the WorkOS  login but where that login flow
 * fails has changed with the environment.
 */
export function isWebContainer(): boolean {
  // Dynamic require as used here doesn't work with tsx
  if (process.env.CONVEX_RUNNING_LIVE_IN_MONOREPO) {
    return false;
  }
  const dynamicRequire = require;
  if (process.versions.webcontainer === undefined) {
    return false;
  }
  let blitzInternalEnv: unknown;
  try {
    blitzInternalEnv = dynamicRequire("@blitz/internal/env");
    // totally fine for this require to fail
    // eslint-disable-next-line no-empty
  } catch {}
  return blitzInternalEnv !== null && blitzInternalEnv !== undefined;
}

// For (rare) special behaviors based on package.json details.
export async function currentPackageHomepage(
  ctx: Context,
): Promise<string | null> {
  const { parentPackageJson: packageJsonPath } = await findParentConfigs(ctx);
  let packageJson: any;
  try {
    const packageJsonString = ctx.fs.readUtf8File(packageJsonPath);
    packageJson = JSON.parse(packageJsonString);
  } catch (error: any) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Couldn't parse "${packageJsonPath}". Make sure it's a valid JSON. Error: ${error}`,
    });
  }
  const name = packageJson["homepage"];
  if (typeof name !== "string") {
    // wrong type or missing
    return null;
  }
  return name;
}
