import { errors, BaseClient, custom } from "openid-client";
import {
  bigBrainAPI,
  bigBrainFetch,
  logAndHandleFetchError,
  throwingFetch,
  isWebContainer,
  typedPlatformClient,
} from "./utils/utils.js";
import open from "open";
import { chalkStderr } from "chalk";
import { provisionHost } from "./config.js";
import { version } from "../version.js";
import { Context } from "../../bundler/context.js";
import {
  changeSpinner,
  logError,
  logFailure,
  logFinishedStep,
  logMessage,
  logOutput,
  logVerbose,
  logWarning,
  showSpinner,
  stopSpinner,
} from "../../bundler/log.js";
import { Issuer } from "openid-client";
import { hostname } from "os";
import { execSync } from "child_process";
import { promptString, promptYesNo } from "./utils/prompts.js";
import {
  GlobalConfig,
  findInstanceForDirectory,
  isMemberId,
  formatPathForPrinting,
  globalConfigPath,
  modifyGlobalConfig,
  defaultAccountId,
  readGlobalConfig,
  updateGlobalConfig,
  withAccount,
  withRekeyedAccount,
  savedAccounts,
  withAccounts,
  shouldRevokeReplacedTokens,
} from "./utils/globalConfig.js";
import { updateBigBrainAuthAfterLogin } from "./deploymentSelection.js";
import { getTeamsForUser } from "./api.js";

// Per https://github.com/panva/node-openid-client/tree/main/docs#customizing
custom.setHttpOptionsDefaults({
  timeout: parseInt(process.env.OPENID_CLIENT_TIMEOUT || "10000"),
});

/**
 * Whether Big Brain accepts this credential. Takes the header explicitly so
 * callers can check a credential other than the one `ctx` resolved to.
 */
export async function isAuthorizedHeader(
  ctx: Context,
  header: string,
): Promise<boolean> {
  try {
    const resp = await fetch(`${provisionHost}/api/authorize`, {
      method: "HEAD",
      headers: {
        Authorization: header,
        "Convex-Client": `npm-cli-${version}`,
      },
    });
    // Don't throw an error if this request returns a non-200 status.
    // Big Brain responds with a variety of error codes -- 401 if the token is correctly-formed but not valid, and either 400 or 500 if the token is ill-formed.
    // We only care if this check returns a 200 code (so we can skip logging in again) -- any other errors should be silently skipped and we'll run the whole login flow again.
    return resp.status === 200;
  } catch (e: any) {
    // This `catch` block should only be hit if a network error was encountered
    logError(
      `Unexpected error when authorizing - are you connected to the internet?`,
    );
    return await logAndHandleFetchError(ctx, e);
  }
}

export async function checkAuthorization(
  ctx: Context,
  acceptOptIns: boolean,
): Promise<boolean> {
  const header = ctx.bigBrainAuth()?.header ?? null;
  if (header === null) {
    return false;
  }
  if (!(await isAuthorizedHeader(ctx, header))) {
    return false;
  }

  // Check that we have optin as well
  const shouldContinue = await optins(ctx, acceptOptIns);
  if (!shouldContinue) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: null,
    });
  }
  return true;
}

async function performDeviceAuthorization(
  ctx: Context,
  authClient: BaseClient,
  shouldOpen: boolean,
  vercel?: boolean,
  vercelOverride?: string,
  acceptDefaults?: boolean,
): Promise<string> {
  // Device authorization flow follows this guide: https://github.com/auth0/auth0-device-flow-cli-sample/blob/9f0f3b76a6cd56ea8d99e76769187ea5102d519d/cli.js
  // License: MIT License
  // Copyright (c) 2019 Auth0 Samples
  /*
  The MIT License (MIT)

  Copyright (c) 2019 Auth0 Samples

  Permission is hereby granted, free of charge, to any person obtaining a copy
  of this software and associated documentation files (the "Software"), to deal
  in the Software without restriction, including without limitation the rights
  to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
  copies of the Software, and to permit persons to whom the Software is
  furnished to do so, subject to the following conditions:

  The above copyright notice and this permission notice shall be included in all
  copies or substantial portions of the Software.

  THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
  IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
  FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
  AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
  LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
  OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
  SOFTWARE.
  */

  // Device Authorization Request - https://tools.ietf.org/html/rfc8628#section-3.1
  // Get authentication URL
  let handle;
  try {
    handle = await authClient.deviceAuthorization();
  } catch {
    // We couldn't get verification URL from the auth provider, proceed with manual auth
    return promptString(ctx, {
      message:
        "Open https://dashboard.convex.dev/auth, log in and paste the token here:",
    });
  }

  // Device Authorization Response - https://tools.ietf.org/html/rfc8628#section-3.2
  // Open authentication URL
  const { verification_uri_complete, user_code, expires_in } = handle;

  // Construct Vercel URL if --vercel flag is used
  const urlToOpen = vercel
    ? `https://vercel.com/sso/integrations/${vercelOverride || "convex"}?url=${verification_uri_complete}`
    : verification_uri_complete;

  logMessage(
    `Visit ${urlToOpen} to finish logging in.\n` +
      `You should see the following code which expires in ${
        expires_in % 60 === 0
          ? `${expires_in / 60} minutes`
          : `${expires_in} seconds`
      }: ${user_code}`,
  );
  if (shouldOpen && !acceptDefaults) {
    shouldOpen = await promptYesNo(ctx, {
      message: `Open the browser?`,
      default: true,
    });
  }

  if (shouldOpen) {
    showSpinner(`Opening ${urlToOpen} in your browser to log in...\n`);
    try {
      const p = await open(urlToOpen);
      p.once("error", () => {
        changeSpinner(`Manually open ${urlToOpen} in your browser to log in.`);
      });
      changeSpinner("Waiting for the confirmation...");
    } catch {
      logError(chalkStderr.red(`Unable to open browser.`));
      changeSpinner(`Manually open ${urlToOpen} in your browser to log in.`);
    }
  } else {
    showSpinner(`Open ${urlToOpen} in your browser to log in.`);
  }

  // Device Access Token Request - https://tools.ietf.org/html/rfc8628#section-3.4
  // Device Access Token Response - https://tools.ietf.org/html/rfc8628#section-3.5
  try {
    const tokens = await handle.poll();
    // Stop "Waiting for the confirmation..." now, or it keeps redrawing over
    // any prompt shown before the next finished step.
    stopSpinner();
    if (typeof tokens.access_token === "string") {
      return tokens.access_token;
    } else {
      // Unexpected error
      // eslint-disable-next-line no-restricted-syntax
      throw Error("Access token is missing");
    }
  } catch (err: any) {
    switch (err.error) {
      case "access_denied": // end-user declined the device confirmation prompt, consent or rules failed
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: "Access denied.",
          errForSentry: err,
        });
      case "expired_token": // end-user did not complete the interaction in time
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: "Device flow expired.",
          errForSentry: err,
        });
      default: {
        const message =
          err instanceof errors.OPError
            ? `Error = ${err.error}; error_description = ${err.error_description}`
            : `Login failed with error: ${err}`;
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: message,
          errForSentry: err,
        });
      }
    }
  }
}

async function performPasswordAuthentication(
  ctx: Context,
  clientId: string,
  username: string,
  password: string,
): Promise<string> {
  if (!process.env.WORKOS_API_SECRET) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "WORKOS_API_SECRET environment variable is not set",
    });
  }

  // Unfortunately, `openid-client` doesn't support the resource owner password credentials flow so we need to manually send the requests.
  const options: Parameters<typeof throwingFetch>[1] = {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({
      grant_type: "password",
      email: username,
      password: password,
      client_id: clientId,
      client_secret: process.env.WORKOS_API_SECRET,
    }),
  };

  try {
    const response = await throwingFetch(
      "https://apiauth.convex.dev/user_management/authenticate",
      options,
    );
    const data = await response.json();
    if (typeof data.access_token === "string") {
      return data.access_token;
    } else {
      // Unexpected error
      // eslint-disable-next-line no-restricted-syntax
      throw Error("Access token is missing");
    }
  } catch (err: any) {
    logFailure(`Password flow failed: ${err}`);
    if (err.response) {
      logError(chalkStderr.red(`${JSON.stringify(err.response.data)}`));
    }
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      errForSentry: err,
      printedMessage: null,
    });
  }
}

export async function performLogin(
  ctx: Context,
  {
    overrideAuthUrl,
    overrideAuthClient,
    overrideAuthUsername,
    overrideAuthPassword,
    overrideAccessToken,
    loginFlow,
    open,
    acceptOptIns,
    dumpAccessToken,
    deviceName: deviceNameOverride,
    anonymousId,
    vercel,
    vercelOverride,
    account,
    accountDescription,
    nameNewAccount,
    acceptDefaults,
  }: {
    overrideAuthUrl?: string | undefined;
    overrideAuthClient?: string | undefined;
    overrideAuthUsername?: string | undefined;
    overrideAuthPassword?: string | undefined;
    overrideAccessToken?: string | undefined;
    loginFlow?: "auto" | "paste" | "poll" | undefined;
    // default `true`
    open?: boolean | undefined;
    // default `false`
    acceptOptIns?: boolean | undefined;
    dumpAccessToken?: boolean | undefined;
    deviceName?: string | undefined;
    anonymousId?: string | undefined;
    vercel?: boolean | undefined;
    vercelOverride?: string | undefined;
    // Save the token as this account instead of the single global token.
    account?: string | undefined;
    accountDescription?: string | undefined;
    // Save the token as a new account under its Convex member id.
    nameNewAccount?: boolean | undefined;
    // Take the default answer instead of prompting.
    acceptDefaults?: boolean | undefined;
  } = {},
): Promise<string | null> {
  loginFlow = loginFlow || "auto";
  // Get access token from big-brain
  // Default the device name to the hostname, but allow the user to change this if the terminal is interactive.
  // On Macs, the `hostname()` may be a weirdly-truncated form of the computer name. Attempt to read the "real" name before falling back to hostname.
  let deviceName = deviceNameOverride ?? "";
  if (!deviceName && process.platform === "darwin") {
    try {
      deviceName = execSync("scutil --get ComputerName").toString().trim();
    } catch {
      // Just fall back to the hostname default below.
    }
  }
  if (!deviceName) {
    deviceName = hostname();
  }
  if (!deviceNameOverride) {
    logMessage(
      chalkStderr.bold(
        `Welcome to developing with Convex, let's get you logged in.`,
      ),
    );
    if (!acceptDefaults) {
      deviceName = await promptString(ctx, {
        message: "Device name:",
        default: deviceName,
      });
    }
  }

  const issuer = overrideAuthUrl ?? "https://auth.convex.dev";
  const clientId = overrideAuthClient ?? "HFtA247jp9iNs08NTLIB7JsNPMmRIyfi";
  let accessToken: string;

  if (overrideAccessToken) {
    // Access token was supplied directly.
    accessToken = overrideAccessToken;
  } else if (overrideAuthUsername && overrideAuthPassword) {
    // Username/Password auth
    accessToken = await performPasswordAuthentication(
      ctx,
      clientId,
      overrideAuthUsername,
      overrideAuthPassword,
    );
  } else if (
    loginFlow === "paste" ||
    (loginFlow === "auto" && isWebContainer())
  ) {
    accessToken = await promptString(ctx, {
      message:
        "Open https://dashboard.convex.dev/auth, log in and paste the token here:",
    });
  } else {
    // Device authorization flow. Contact OIDC issuer.
    let authIssuer;
    try {
      authIssuer = await Issuer.discover(issuer);
    } catch {
      // Couldn't contact https://auth.convex.dev/.well-known/openid-configuration,
      // proceed with manual auth.
      authIssuer = undefined;
    }
    if (authIssuer) {
      const authClient = new authIssuer.Client({
        client_id: clientId,
        token_endpoint_auth_method: "none",
        id_token_signed_response_alg: "RS256",
      });
      accessToken = await performDeviceAuthorization(
        ctx,
        authClient,
        open ?? true,
        vercel,
        vercelOverride,
        acceptDefaults,
      );
    } else {
      accessToken = await promptString(ctx, {
        message:
          "Open https://dashboard.convex.dev/auth, log in and paste the token here:",
      });
    }
  }

  if (dumpAccessToken) {
    logOutput(`${accessToken}`);
    return await ctx.crash({
      exitCode: 0,
      errorType: "fatal",
      printedMessage: null,
    });
  }

  // Exchange the WorkOS access token for a Convex personal access token.
  ctx._updateBigBrainAuth({
    accessToken: accessToken,
    kind: "accessToken",
    header: `Bearer ${accessToken}`,
  });
  // Work out which account this login is before creating its token, so
  // quitting at a prompt doesn't leave a token that isn't saved anywhere.
  const profile = await fetchProfile(accessToken);
  const existingConfig = readGlobalConfig(ctx);
  const hasAccounts = existingConfig?.accounts !== undefined;
  // The saved account this login refreshes: the one asked for, or the one
  // this directory uses.
  const replacing =
    account ??
    (hasAccounts && !nameNewAccount
      ? (findInstanceForDirectory(existingConfig, process.cwd())?.account ??
        defaultAccountId(existingConfig) ??
        undefined)
      : undefined);
  let accountKey: string | undefined;
  let description = accountDescription;
  if (account !== undefined || nameNewAccount || hasAccounts) {
    if (profile === null) {
      if (replacing === undefined) {
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage:
            "Couldn't read the Convex member for this login, so it can't be saved as a separate account. Try again, or run `npx convex login` without --account.",
        });
      }
      accountKey = replacing;
    } else {
      accountKey = String(profile.id);
      logMessage(`Logged in as ${profile.email} (member ${accountKey}).`);
      const alreadySaved = existingConfig?.accounts?.[accountKey] !== undefined;
      if (
        replacing !== undefined &&
        replacing !== accountKey &&
        isMemberId(replacing)
      ) {
        logWarning(
          `This login is member ${accountKey}, not member ${replacing}, so it is saved as a separate account and ${replacing} keeps its token.`,
        );
      } else if (nameNewAccount && alreadySaved) {
        logWarning(
          `Member ${accountKey} is already saved, so its token was refreshed. To add a different account, log out at https://dashboard.convex.dev first, then run this again.`,
        );
      }
      if (!alreadySaved && description === undefined) {
        description = await promptForDescription(ctx, { acceptDefaults });
      }
    }
  }
  // An old token's member can't be checked once it's revoked, so ask before
  // giving its role and directories to whoever just logged in.
  let rekeyFrom: string[] = [];
  const saved = new Map(savedAccounts(existingConfig));
  const replacedAccount =
    replacing !== undefined && !isMemberId(replacing)
      ? saved.get(replacing)
      : undefined;
  if (
    profile !== null &&
    replacing !== undefined &&
    accountKey !== undefined &&
    replacedAccount !== undefined
  ) {
    const wasDefault = replacedAccount.isDefault === true;
    const oldToken = replacedAccount.accessToken;
    // A working old token can be matched to this login: both list the same
    // member's personal access tokens.
    const sameMember = await isSameMember(oldToken, accessToken);
    let handOver: boolean;
    if (sameMember === true) {
      logMessage(
        `The "${replacing}" token belongs to member ${accountKey}; it is now saved under that id.`,
      );
      handOver = true;
    } else if (sameMember === false) {
      logMessage(
        `The "${replacing}" token belongs to a different member than ${profile.email} (member ${accountKey}), so both are kept.`,
      );
      handOver = false;
    } else {
      handOver =
        acceptDefaults || !process.stdin.isTTY
          ? false
          : await promptYesNo(ctx, {
              message: `The "${replacing}" token was saved before its member was recorded, and it can't be checked. You logged in as ${profile.email} (member ${accountKey}). Replace it with member ${accountKey}${wasDefault ? ", making it the default account" : ""} and moving its directories?`,
              default: true,
            });
    }
    if (handOver) {
      rekeyFrom = [replacing];
    } else if (sameMember !== false) {
      logMessage(
        `Kept the "${replacing}" token as a separate account. Remove it with ${chalkStderr.bold(`npx convex logout --account ${replacing}`)}.`,
      );
    }
  }
  const response = await typedPlatformClient(ctx).POST(
    "/create_personal_access_token",
    {
      // `anonymousId` links a prior anonymous session to this account. It's
      // intentionally absent from the generated platform API types,
      // so cast to pass it through.
      body: { name: deviceName, anonymousId } as { name: string },
    },
  );
  const newAccessToken = response.data!.accessToken;
  // Any other unidentified token of this member merges into its account too.
  if (accountKey !== undefined && isMemberId(accountKey)) {
    for (const from of await unidentifiedTokensOfMember(
      existingConfig,
      newAccessToken,
    )) {
      if (!rekeyFrom.includes(from)) {
        logMessage(describeMerge(existingConfig, from, accountKey));
        rekeyFrom.push(from);
      }
    }
  }
  // Tokens this login replaces: the account's previous token and those of
  // any unidentified entries merged into it.
  const replacedTokens =
    accountKey === undefined
      ? []
      : [accountKey, ...rekeyFrom].map((id) => saved.get(id)?.accessToken);
  let savedAccountId: string | null;
  try {
    savedAccountId = await saveAccessToken(ctx, newAccessToken, {
      account: accountKey,
      description,
      email: profile?.email,
      rekeyFrom,
    });
    const path = globalConfigPath();
    logFinishedStep(
      savedAccountId === null
        ? `Saved credentials to ${formatPathForPrinting(path)}`
        : `Saved credentials for account ${savedAccountId} to ${formatPathForPrinting(path)}`,
    );
  } catch (err: unknown) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      errForSentry: err,
      printedMessage: null,
    });
  }

  if (savedAccountId !== null) {
    await revokeReplacedTokens(ctx, replacedTokens, newAccessToken);
  }

  logVerbose(`performLogin: updating big brain auth after login`);
  await updateBigBrainAuthAfterLogin(ctx, newAccessToken);

  logVerbose(`performLogin: checking opt ins, acceptOptIns: ${acceptOptIns}`);
  // Do opt in to TOS and Privacy Policy stuff
  const shouldContinue = await optins(ctx, acceptOptIns ?? false);
  if (!shouldContinue) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: null,
    });
  }

  if (vercel) {
    await promptJoinVercelTeams(ctx);
  }
  return savedAccountId;
}

type PotentialVercelTeam = {
  teamId: number;
  teamName: string;
  teamSlug: string;
  planId: string;
  planName: string;
  pricingNotice: string | null;
};

// After `--vercel` login, surface any Vercel-marketplace teams the user has
// access to but isn't yet a member of, and prompt to join each.
async function promptJoinVercelTeams(ctx: Context): Promise<void> {
  const fetch = bigBrainFetch(ctx);
  let teams: PotentialVercelTeam[];
  try {
    const res = await fetch(
      new URL("vercel/potential_teams", `${provisionHost}/`),
    );
    if (!res.ok) {
      logVerbose(
        `vercel/potential_teams returned ${res.status}; skipping team-join prompt`,
      );
      return;
    }
    teams = await res.json();
  } catch (err) {
    logVerbose(`Failed to fetch potential Vercel teams: ${String(err)}`);
    return;
  }
  if (teams.length === 0) return;

  for (const [index, team] of teams.entries()) {
    const displayName = team.teamName.replace(/ \(Vercel\)$/, "");
    const counter = teams.length > 1 ? `[${index + 1}/${teams.length}] ` : "";
    const lines = [
      chalkStderr.bold(`${counter}You've been invited to join ${displayName}`) +
        ` (${team.planName}) through the Vercel marketplace.`,
    ];
    if (team.pricingNotice) {
      lines.push(chalkStderr.yellow(team.pricingNotice));
    }
    lines.push(`Join "${displayName}"?`);
    const join = await promptYesNo(ctx, {
      message: `${lines.join("\n")}`,
      default: true,
    });
    if (!join) continue;
    try {
      await fetch(
        new URL(
          `vercel/potential_teams/${team.teamId}/join`,
          `${provisionHost}/`,
        ),
        { method: "POST" },
      );
      logFinishedStep(`Joined ${displayName}`);
    } catch (err) {
      logFailure(`Failed to join ${displayName}: ${String(err)}`);
    }
  }
}

/// There are fields like version, but we keep them opaque
type OptIn = Record<string, unknown>;

type OptInToAccept = {
  optIn: OptIn;
  message: string;
};

type AcceptOptInsArgs = {
  optInsAccepted: OptIn[];
};

// Returns whether we can proceed or not.
async function optins(ctx: Context, acceptOptIns: boolean): Promise<boolean> {
  const bbAuth = ctx.bigBrainAuth();
  if (bbAuth === null) {
    // This should never happen, but if we're not even logged in, we can't proceed.
    return false;
  }
  switch (bbAuth.kind) {
    case "accessToken":
      break;
    case "deploymentKey":
    case "projectKey":
    case "previewDeployKey":
      // If we have a key configured as auth, we do not need to check opt ins.
      return true;
    default: {
      bbAuth satisfies never;
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        errForSentry: `Unexpected auth kind ${(bbAuth as any).kind}`,
        printedMessage: "Hit an unexpected error while logging in.",
      });
    }
  }
  const data = await bigBrainAPI({
    ctx,
    method: "POST",
    path: "check_opt_ins",
  });
  if (data.optInsToAccept.length === 0) {
    return true;
  }
  for (const optInToAccept of data.optInsToAccept) {
    const confirmed =
      acceptOptIns ||
      (await promptYesNo(ctx, {
        message: optInToAccept.message,
      }));
    if (!confirmed) {
      logFailure("Please accept the Terms of Service to use Convex.");
      return Promise.resolve(false);
    }
  }

  const optInsAccepted = data.optInsToAccept.map((o: OptInToAccept) => o.optIn);
  const args: AcceptOptInsArgs = { optInsAccepted };
  await bigBrainAPI({
    ctx,
    method: "POST",
    path: "accept_opt_ins",
    data: args,
  });
  return true;
}

export type ConvexProfile = {
  // Unique per Convex member. Several members can share an email.
  id: number | string;
  email: string;
  name: string | null;
};

/**
 * The Convex member a credential belongs to, from the same profile endpoint
 * the dashboard uses. Null if the credential is rejected or the profile can't
 * be read.
 */
async function fetchProfile(token: string): Promise<ConvexProfile | null> {
  try {
    const resp = await convexApi(token, "/api/dashboard/profile");
    if (resp.status !== 200) {
      logVerbose(`Couldn't read the Convex profile: ${resp.status}`);
      return null;
    }
    const profile = await resp.json();
    if (
      (typeof profile.id !== "number" && typeof profile.id !== "string") ||
      typeof profile.email !== "string"
    ) {
      return null;
    }
    return {
      id: profile.id,
      email: profile.email,
      name: typeof profile.name === "string" ? profile.name : null,
    };
  } catch (err) {
    logVerbose(`Couldn't read the Convex profile: ${err as any}`);
    return null;
  }
}

/**
 * A request to the Convex API, authorized as the owner of `token`. `path`
 * starts with `/api` or `/v1`.
 */
export async function convexApi(
  token: string,
  path: string,
  init: { method?: "GET" | "POST"; body?: unknown } = {},
): Promise<Response> {
  return await fetch(`${provisionHost}${path}`, {
    method: init.method ?? "GET",
    headers: {
      Authorization: `Bearer ${token}`,
      "Convex-Client": `npm-cli-${version}`,
      ...(init.body !== undefined
        ? { "Content-Type": "application/json" }
        : {}),
    },
    ...(init.body !== undefined ? { body: JSON.stringify(init.body) } : {}),
  });
}

/** The ids of the personal access tokens a token's member owns. */
async function personalAccessTokenIds(
  token: string,
): Promise<Set<string> | null> {
  try {
    const resp = await convexApi(
      token,
      "/v1/list_personal_access_tokens?limit=100",
    );
    if (resp.status !== 200) {
      return null;
    }
    const body = await resp.json();
    if (!Array.isArray(body.items)) {
      return null;
    }
    return new Set(body.items.map((item: { id: unknown }) => String(item.id)));
  } catch {
    return null;
  }
}

/**
 * Whether two token lists belong to the same member. Null if either can't be
 * read. `older` always contains its own token, so an empty `older` list means
 * the response can't be trusted.
 */
function sameMemberByTokenLists(
  older: Set<string> | null,
  newer: Set<string> | null,
): boolean | null {
  if (older === null || newer === null || older.size === 0) {
    return null;
  }
  return [...older].some((id) => newer.has(id));
}

/**
 * Whether two tokens belong to the same Convex member, judged by the
 * personal access tokens each can list. Null if either list can't be read,
 * for example because the older token was revoked.
 */
async function isSameMember(
  olderToken: string,
  newerToken: string,
): Promise<boolean | null> {
  const [older, newer] = await Promise.all([
    personalAccessTokenIds(olderToken),
    personalAccessTokenIds(newerToken),
  ]);
  return sameMemberByTokenLists(older, newer);
}

/**
 * A note shown next to a new account, defaulting to its teams so accounts
 * that share an email are easy to tell apart. Uses the credentials in `ctx`.
 */
async function promptForDescription(
  ctx: Context,
  opts: { acceptDefaults?: boolean | undefined },
): Promise<string | undefined> {
  const teams = await getTeamsForUser(ctx);
  const suggested = teams.map((team) => team.slug).join(", ");
  if (opts.acceptDefaults || !process.stdin.isTTY) {
    return suggested === "" ? undefined : suggested;
  }
  const answer = await promptString(ctx, {
    message: "Description for this account:",
    default: suggested,
  });
  return answer === "" ? undefined : answer;
}

function tokenCount(n: number) {
  return `${n} replaced token${n === 1 ? "" : "s"}`;
}

/**
 * Revoke personal access tokens that newer tokens replaced on this machine,
 * using `authToken`, a current token of the same member, unless the
 * `revoke-replaced-tokens` setting is off. Tokens still saved anywhere in the
 * config are kept. A token that is already revoked is skipped quietly.
 */
async function revokeReplacedTokens(
  ctx: Context,
  tokens: (string | undefined)[],
  authToken: string,
): Promise<void> {
  const config = readGlobalConfig(ctx);
  const stillSaved = new Set([
    config?.accessToken,
    ...Object.values(config?.accounts ?? {}).map(
      (account) => account.accessToken,
    ),
  ]);
  const replaced = [...new Set(tokens)].filter(
    (token): token is string =>
      token !== undefined && token !== authToken && !stillSaved.has(token),
  );
  if (replaced.length === 0) {
    return;
  }
  if (!shouldRevokeReplacedTokens(config)) {
    const it = replaced.length === 1 ? "it" : "them";
    logMessage(
      `Kept ${tokenCount(replaced.length)} active on Convex (revoke-replaced-tokens is off). Delete ${it} in the dashboard if you no longer use ${it}.`,
    );
    return;
  }
  let revoked = 0;
  for (const token of replaced) {
    try {
      const resp = await convexApi(
        authToken,
        "/v1/delete_personal_access_token",
        { method: "POST", body: { id: token } },
      );
      if (resp.ok) {
        revoked++;
      } else {
        logVerbose(`Didn't revoke a replaced token: ${resp.status}`);
      }
    } catch (err) {
      logVerbose(`Didn't revoke a replaced token: ${err as any}`);
    }
  }
  if (revoked > 0) {
    logMessage(
      `Revoked ${tokenCount(revoked)} on Convex. To keep replaced tokens, run \`npx convex account settings revoke-replaced-tokens false\`.`,
    );
  }
}

/**
 * Keys of the unidentified tokens in `config` that belong to the same member
 * as `memberToken`: the old token shows up in the member's own list of
 * personal access tokens. Unreadable or revoked tokens never match.
 */
async function unidentifiedTokensOfMember(
  config: GlobalConfig | null,
  memberToken: string,
): Promise<string[]> {
  const unidentified = savedAccounts(config)
    .filter(([id]) => !isMemberId(id))
    .map(([id, account]) => [id, account.accessToken] as const);
  if (unidentified.length === 0) {
    return [];
  }
  const memberTokenIds = await personalAccessTokenIds(memberToken);
  if (memberTokenIds === null) {
    return [];
  }
  const matches = await Promise.all(
    unidentified.map(async ([id, token]) =>
      sameMemberByTokenLists(
        await personalAccessTokenIds(token),
        memberTokenIds,
      ) === true
        ? id
        : null,
    ),
  );
  return matches.filter((id): id is string => id !== null);
}

function describeMerge(
  config: GlobalConfig | null,
  from: string,
  memberId: string,
): string {
  const wasDefault =
    config?.accounts === undefined || config.accounts[from]?.isDefault === true;
  const email = config?.accounts?.[memberId]?.email;
  return `Identified the "${from}" token: it belongs to member ${memberId}${email ? ` <${email}>` : ""}. It is now listed under that member${wasDefault ? ", which stays the default account" : ""}.`;
}

/**
 * Merge unidentified tokens into the saved member they belong to, found by
 * comparing personal access token lists. Needs no login, so listings can
 * tidy up accounts saved before member ids were recorded.
 */
export async function identifyLegacyAccounts(ctx: Context): Promise<void> {
  const config = readGlobalConfig(ctx);
  const members = Object.entries(config?.accounts ?? {}).filter(([id]) =>
    isMemberId(id),
  );
  if (
    members.length === 0 ||
    !Object.keys(config?.accounts ?? {}).some((id) => !isMemberId(id))
  ) {
    return;
  }
  const merges = new Map<string, string>();
  for (const [memberId, account] of members) {
    for (const from of await unidentifiedTokensOfMember(
      config,
      account.accessToken,
    )) {
      if (!merges.has(from)) {
        merges.set(from, memberId);
      }
    }
  }
  if (merges.size === 0) {
    return;
  }
  for (const [from, memberId] of merges) {
    logMessage(describeMerge(config, from, memberId));
  }
  await updateGlobalConfig(ctx, (current) => {
    let next = current ?? {};
    for (const [from, memberId] of merges) {
      next = withRekeyedAccount(next, from, memberId);
    }
    return next;
  });
  for (const [from, memberId] of merges) {
    await revokeReplacedTokens(
      ctx,
      [config!.accounts![from].accessToken],
      config!.accounts![memberId].accessToken,
    );
  }
}

/**
 * Store a new personal access token and return the account it was saved
 * under, or null when the config holds a single token.
 */
async function saveAccessToken(
  ctx: Context,
  accessToken: string,
  opts: {
    account: string | undefined;
    description: string | undefined;
    email: string | undefined;
    rekeyFrom: string[];
  },
): Promise<string | null> {
  if (opts.account === undefined) {
    await modifyGlobalConfig(ctx, { accessToken });
    return null;
  }
  const id = opts.account;
  await updateGlobalConfig(ctx, (config) => {
    // A single-token config becomes the `unidentified` account first, so the
    // rekey below can move its directories.
    let next = withAccounts(config);
    for (const from of opts.rekeyFrom) {
      next = withRekeyedAccount(next, from, id);
    }
    return withAccount(next, id, {
      accessToken,
      description: opts.description,
      email: opts.email,
    });
  });
  return id;
}

export async function ensureLoggedIn(
  ctx: Context,
  options?: {
    message?: string | undefined;
    overrideAuthUrl?: string | undefined;
    overrideAuthClient?: string | undefined;
    overrideAuthUsername?: string | undefined;
    overrideAuthPassword?: string | undefined;
  },
) {
  const isLoggedIn = await checkAuthorization(ctx, false);
  if (!isLoggedIn) {
    if (options?.message) {
      logMessage(options.message);
    }
    await performLogin(ctx, {
      acceptOptIns: false,
      overrideAuthUrl: options?.overrideAuthUrl,
      overrideAuthClient: options?.overrideAuthClient,
      overrideAuthUsername: options?.overrideAuthUsername,
      overrideAuthPassword: options?.overrideAuthPassword,
    });
  }
}
