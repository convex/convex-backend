import { ProjectSelection } from "./deploymentSelection.js";
import { Context } from "../../bundler/context.js";
import { logMessage, logWarning } from "../../bundler/log.js";
import { provisionHost } from "./config.js";
import { version } from "../version.js";
import {
  isAnonymousDeployment,
  getDeploymentTypeFromConfiguredDeployment,
  stripDeploymentTypePrefix,
} from "./deployment.js";
import { CONVEX_DEPLOYMENT_ENV_VAR_NAME } from "./utils/utils.js";
import { promptYesNo } from "./utils/prompts.js";
import { loadProjectEnvironmentVariables } from "./envvars.js";
import {
  AccountCredentials,
  AccountSelection,
  readAccountCredentials,
  writeAccountDirectory,
} from "./accounts.js";

export async function accountApi(
  token: string,
  pathname: string,
  body?: unknown,
) {
  return await fetch(`${provisionHost}${pathname}`, {
    method: body === undefined ? "GET" : "POST",
    headers: {
      Authorization: `Bearer ${token}`,
      "Convex-Client": `npm-cli-${version}`,
      ...(body === undefined ? {} : { "Content-Type": "application/json" }),
    },
    ...(body === undefined ? {} : { body: JSON.stringify(body) }),
    signal: AbortSignal.timeout(10_000),
  });
}

export async function deploymentAccess(
  token: string,
  deploymentName: string,
): Promise<"yes" | "no" | "unknown"> {
  try {
    const response = await accountApi(
      token,
      `/v1/deployments/${encodeURIComponent(deploymentName)}`,
    );
    if (response.ok) return "yes";
    return [401, 403, 404].includes(response.status) ? "no" : "unknown";
  } catch {
    return "unknown";
  }
}

export async function projectAccess(
  token: string,
  target: ProjectSelection,
): Promise<"yes" | "no" | "unknown"> {
  if (target.kind === "deploymentName")
    return deploymentAccess(token, target.deploymentName);
  if (target.kind !== "teamAndProjectSlugs") return "unknown";
  try {
    const response = await accountApi(
      token,
      `/v1/teams/${encodeURIComponent(target.teamSlug)}/projects/${encodeURIComponent(target.projectSlug)}`,
    );
    if (response.ok) return "yes";
    return [401, 403, 404].includes(response.status) ? "no" : "unknown";
  } catch {
    return "unknown";
  }
}

function configuredCloudDeployment(): string | null {
  const configured = process.env[CONVEX_DEPLOYMENT_ENV_VAR_NAME]?.trim();
  if (
    !configured ||
    getDeploymentTypeFromConfiguredDeployment(configured) === "local"
  )
    return null;
  const name = stripDeploymentTypePrefix(configured);
  return isAnonymousDeployment(name) ? null : name;
}

export async function bindAccountToDirectory(
  ctx: Context,
  name: string,
  force = false,
  target?: ProjectSelection,
): Promise<boolean> {
  const credentials = await readAccountCredentials(ctx, {
    name,
    source: "account selection",
  });
  loadProjectEnvironmentVariables();
  const deployment = configuredCloudDeployment();
  const selectedTarget =
    target ??
    (deployment === null
      ? null
      : {
          kind: "deploymentName" as const,
          deploymentName: deployment,
          deploymentType: null,
        });
  if (!force && selectedTarget !== null) {
    const label =
      selectedTarget.kind === "teamAndProjectSlugs"
        ? `project "${selectedTarget.teamSlug}/${selectedTarget.projectSlug}"`
        : `deployment "${selectedTarget.kind === "deploymentName" ? selectedTarget.deploymentName : "configured"}"`;
    const access = await projectAccess(
      credentials!.accessToken,
      selectedTarget,
    );
    if (access !== "yes") {
      logWarning(
        access === "no"
          ? `Account "${name}" cannot access ${label}.`
          : `Could not verify whether account "${name}" can access ${label}.`,
      );
      if (
        !process.stdin.isTTY ||
        !(await promptYesNo(ctx, {
          message: `Use account "${name}" here anyway?`,
          default: false,
        }))
      ) {
        logMessage(
          `Directory selection unchanged. To select this account anyway, run \`npx convex account select ${name} --force\`.`,
        );
        return false;
      }
    }
  }
  await writeAccountDirectory(ctx, name);
  return true;
}

export async function accountLoginMetadata(
  ctx: Context,
  account: AccountSelection,
  loginToken: string,
  previous: AccountCredentials | null,
): Promise<Omit<AccountCredentials, "accessToken">> {
  let identity: { memberId: string; email: string } | null = null;
  try {
    const response = await accountApi(loginToken, "/api/dashboard/profile");
    if (response.ok) {
      const data = await response.json();
      if (
        (typeof data.id === "number" || typeof data.id === "string") &&
        typeof data.email === "string"
      ) {
        identity = { memberId: String(data.id), email: data.email };
      }
    }
  } catch {
    // Older authentication tokens can be valid without access to the dashboard profile API.
  }
  const sameAccount =
    previous !== null && (previous.memberId === undefined || identity === null)
      ? await tokensBelongToSameAccount(previous.accessToken, loginToken)
      : null;
  // A saved login that recorded no identity and whose token the server has
  // rejected identifies no account, so this login takes its place.
  const replacesUnidentifiedLogin =
    previous?.memberId === undefined && sameAccount === "olderRejected";
  if (
    previous !== null &&
    !replacesUnidentifiedLogin &&
    (previous.memberId === undefined || identity === null
      ? sameAccount !== true
      : identity.memberId !== previous.memberId)
  ) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage:
        sameAccount !== false &&
        (previous.memberId === undefined || identity === null)
          ? `Could not verify the identity of saved account "${account.name}". To replace this saved login, run \`npx convex logout --account ${account.name}\` followed by \`npx convex login --account ${account.name}\`. Choose the intended account when logging in.`
          : `The login does not match saved account "${account.name}". Log in with the matching account, or choose another name with \`npx convex login --account <name>\`.`,
    });
  }
  return {
    ...(identity ??
      (previous?.memberId === undefined
        ? {}
        : { memberId: previous.memberId, email: previous.email })),
  };
}

async function tokensBelongToSameAccount(
  older: string,
  newer: string,
): Promise<boolean | "olderRejected" | null> {
  try {
    const responses = await Promise.all(
      [older, newer].map((token) =>
        accountApi(token, "/v1/list_personal_access_tokens?limit=100"),
      ),
    );
    if (responses[0].status === 401 && responses[1].ok) return "olderRejected";
    if (responses.some((response) => !response.ok)) return null;
    const lists = await Promise.all(
      responses.map((response) => response.json()),
    );
    if (
      lists.some(
        (list) => !Array.isArray(list.items) || list.items.length === 0,
      )
    )
      return null;
    const ids = new Set(
      lists[0].items.map((token: { id: unknown }) => String(token.id)),
    );
    if (
      lists[1].items.some((token: { id: unknown }) => ids.has(String(token.id)))
    )
      return true;
    // Disjoint partial pages cannot establish that the accounts differ.
    return lists.some((list) => list.pagination?.hasMore !== false)
      ? null
      : false;
  } catch {
    return null;
  }
}
