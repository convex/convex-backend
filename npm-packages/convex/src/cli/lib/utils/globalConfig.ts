import { chalkStderr } from "chalk";
import os from "os";
import path from "path";
import { rootDirectory } from "./utils.js";
import { BigBrainAuth, Context } from "../../../bundler/context.js";
import { logError, logVerbose } from "../../../bundler/log.js";
import { z } from "zod";

export function globalConfigPath(): string {
  return path.join(rootDirectory(), "config.json");
}

// GlobalConfig is stored in a file that very old versions of Convex also need to access.
// Everything besides accessToken must be optional forever.
// GlobalConfig is deleted on logout. It is primarily used for the accessToken.
//
// `accounts` and `instances` hold several logged-in accounts and which project
// directory uses which account. Accounts are keyed by Convex member id, which is
// unique per member, unlike an email, which several members can share. A key
// that isn't a member id (`unidentified`) is a token saved before its member was
// known. While `accounts` is present, the top-level
// `accessToken` mirrors the default account so that older CLI versions (pinned
// by other projects on this machine) keep working as that account.
export type GlobalConfig = {
  accessToken?: string;
  accounts?: Record<string, AccountConfig>;
  instances?: Record<string, InstanceConfig>;
  settings?: GlobalSettings;
};

export type GlobalSettings = {
  // Revoke a personal access token on Convex once a newer token for the same
  // member replaces it here. Unset means true.
  revokeReplacedTokens?: boolean;
};

export const SETTINGS = {
  "revoke-replaced-tokens": {
    key: "revokeReplacedTokens",
    default: true,
    description:
      "Revoke a token on Convex when a newer token for the same member replaces it on this machine",
  },
} as const;

export function shouldRevokeReplacedTokens(
  config: GlobalConfig | null,
): boolean {
  return config?.settings?.revokeReplacedTokens ?? true;
}

export function withSetting(
  config: GlobalConfig,
  key: keyof GlobalSettings,
  value: boolean,
): GlobalConfig {
  return { ...config, settings: { ...config.settings, [key]: value } };
}

export type AccountConfig = {
  accessToken: string;
  email?: string;
  description?: string;
  isDefault?: boolean;
};

export type InstanceConfig = {
  account: string;
};

const schema = z
  .object({
    accessToken: z.string().min(1).optional(),
    accounts: z
      .record(
        z.string(),
        z.object({
          accessToken: z.string().min(1),
          description: z.string().optional(),
          isDefault: z.boolean().optional(),
          // Written by development builds before the default role was named.
          isPrimary: z.boolean().optional(),
          email: z.string().optional(),
        }),
      )
      .optional(),
    instances: z
      .record(z.string(), z.object({ account: z.string().min(1) }))
      .optional(),
    settings: z
      .object({ revokeReplacedTokens: z.boolean().optional() })
      .optional(),
  })
  .refine(
    (config) =>
      config.accessToken !== undefined || config.accounts !== undefined,
    { message: "Expected accessToken or accounts" },
  );

/** The key for a token saved before its member was known. */
export const UNIDENTIFIED_ACCOUNT = "unidentified";

/** Whether an account key is a Convex member id, as opposed to `unidentified`. */
export function isMemberId(id: string): boolean {
  return /^[0-9]+$/.test(id);
}

export function defaultAccountId(config: GlobalConfig): string | null {
  for (const [id, account] of Object.entries(config.accounts ?? {})) {
    if (account.isDefault) {
      return id;
    }
  }
  return null;
}

/**
 * The instance whose directory is `dir` or the closest ancestor of it, so
 * commands run from a subdirectory of a project still find its account.
 */
export function findInstanceForDirectory(
  config: GlobalConfig,
  dir: string,
): { directory: string; account: string } | null {
  const target = path.resolve(dir);
  let best: { directory: string; account: string } | null = null;
  for (const [directory, instance] of Object.entries(config.instances ?? {})) {
    const relative = path.relative(path.resolve(directory), target);
    const isSameOrInside =
      relative === "" ||
      (!relative.startsWith("..") && !path.isAbsolute(relative));
    if (
      isSameOrInside &&
      (best === null ||
        path.resolve(directory).length > path.resolve(best.directory).length)
    ) {
      best = { directory, account: instance.account };
    }
  }
  return best;
}

export type ResolvedAccessToken = {
  accessToken: string;
  // null for a config written before multiple accounts existed.
  accountId: string | null;
  source: "directory" | "default" | "legacy";
};

/**
 * Which account token to use for commands run from `dir`: the account bound
 * to the directory, then the default account, then the top-level token.
 *
 * A directory bound to an account that is no longer logged in resolves to
 * null instead of falling back to the default account, so a command never
 * silently acts as a different account.
 */
export function resolveAccessToken(
  config: GlobalConfig,
  dir: string,
): ResolvedAccessToken | null {
  const legacy: ResolvedAccessToken | null =
    config.accessToken !== undefined
      ? { accessToken: config.accessToken, accountId: null, source: "legacy" }
      : null;
  if (config.accounts === undefined) {
    return legacy;
  }
  const instance = findInstanceForDirectory(config, dir);
  if (instance !== null) {
    const account = config.accounts[instance.account];
    if (account === undefined) {
      logError(
        chalkStderr.red(
          `${formatPathForPrinting(instance.directory)} uses the Convex account "${instance.account}", which is not logged in. Run \`npx convex login --account ${instance.account}\` or \`npx convex account use <id>\`.`,
        ),
      );
      return null;
    }
    return {
      accessToken: account.accessToken,
      accountId: instance.account,
      source: "directory",
    };
  }
  const defaultId = defaultAccountId(config);
  if (defaultId !== null) {
    return {
      accessToken: config.accounts[defaultId].accessToken,
      accountId: defaultId,
      source: "default",
    };
  }
  return legacy;
}

/**
 * Keep exactly one default account while any exist, and mirror its token to
 * the top-level `accessToken` for older CLI versions.
 */
function normalizeAccounts(config: GlobalConfig): GlobalConfig {
  const ids = Object.keys(config.accounts ?? {});
  if (config.accounts === undefined || ids.length === 0) {
    const { accessToken: _accessToken, ...rest } = config;
    return rest;
  }
  const defaultId = defaultAccountId(config) ?? ids[0];
  const accounts: Record<string, AccountConfig> = {};
  for (const [id, account] of Object.entries(config.accounts)) {
    const { isDefault: _isDefault, ...rest } = account;
    accounts[id] = id === defaultId ? { ...rest, isDefault: true } : rest;
  }
  return {
    ...config,
    accessToken: accounts[defaultId].accessToken,
    accounts,
  };
}

/**
 * The saved accounts. A single-token config shows up as the unidentified
 * default account it becomes when converted.
 */
export function savedAccounts(
  config: GlobalConfig | null,
): [string, AccountConfig][] {
  if (config?.accounts !== undefined) {
    return Object.entries(config.accounts);
  }
  return config?.accessToken !== undefined
    ? [[UNIDENTIFIED_ACCOUNT, { accessToken: config.accessToken, isDefault: true }]]
    : [];
}

/** The config with `accounts`, converting a single-token config first. */
export function withAccounts(config: GlobalConfig | null): GlobalConfig {
  const base = config ?? {};
  if (base.accounts !== undefined) {
    return base;
  }
  return normalizeAccounts({
    ...base,
    accounts: Object.fromEntries(savedAccounts(base)),
    instances: base.instances ?? {},
  });
}

export function accessTokenAuth(accessToken: string): BigBrainAuth {
  return {
    kind: "accessToken",
    header: `Bearer ${accessToken}`,
    accessToken,
  };
}

/**
 * Add an account or replace its token. Adding the first account converts a
 * single-account config: its token becomes the `unidentified` account.
 */
export function withAccount(
  config: GlobalConfig | null,
  id: string,
  account: {
    accessToken: string;
    description?: string | undefined;
    email?: string | undefined;
  },
): GlobalConfig {
  const base = withAccounts(config);
  const accounts: Record<string, AccountConfig> = { ...base.accounts };
  accounts[id] = {
    ...accounts[id],
    accessToken: account.accessToken,
    ...(account.description !== undefined
      ? { description: account.description }
      : {}),
    ...(account.email !== undefined ? { email: account.email } : {}),
  };
  return normalizeAccounts({
    ...base,
    accounts,
    instances: base.instances ?? {},
  });
}

/** Remove an account and every directory bound to it. */
export function withoutAccount(config: GlobalConfig, id: string): GlobalConfig {
  const { [id]: _removed, ...accounts } = config.accounts ?? {};
  const instances = Object.fromEntries(
    Object.entries(config.instances ?? {}).filter(
      ([, instance]) => instance.account !== id,
    ),
  );
  return normalizeAccounts({ ...config, accounts, instances });
}

export function withDefaultAccount(
  config: GlobalConfig,
  id: string,
): GlobalConfig {
  const accounts = Object.fromEntries(
    Object.entries(config.accounts ?? {}).map(([accountId, account]) => [
      accountId,
      { ...account, isDefault: accountId === id },
    ]),
  );
  return normalizeAccounts({ ...config, accounts });
}

/**
 * Move an account saved under `from` (usually `unidentified`) to its member id,
 * along with its directories. If the member is already saved, the two merge
 * and the member's own entry wins.
 */
export function withRekeyedAccount(
  config: GlobalConfig,
  from: string,
  to: string,
): GlobalConfig {
  const { [from]: moved, ...accounts } = config.accounts ?? {};
  if (moved === undefined || from === to) {
    return config;
  }
  const merged: AccountConfig = { ...moved, ...accounts[to] };
  if (moved.isDefault || accounts[to]?.isDefault) {
    merged.isDefault = true;
  }
  accounts[to] = merged;
  const instances = Object.fromEntries(
    Object.entries(config.instances ?? {}).map(([directory, instance]) => [
      directory,
      instance.account === from ? { account: to } : instance,
    ]),
  );
  return normalizeAccounts({ ...config, accounts, instances });
}

/** Bind `dir` to an account, or unbind it when `accountId` is null. */
export function withInstance(
  config: GlobalConfig,
  dir: string,
  accountId: string | null,
): GlobalConfig {
  const directory = path.resolve(dir);
  const { [directory]: _previous, ...instances } = config.instances ?? {};
  if (accountId !== null) {
    instances[directory] = { account: accountId };
  }
  return { ...config, instances };
}

export function readGlobalConfig(ctx: Context): GlobalConfig | null {
  const configPath = globalConfigPath();
  let configFile;
  try {
    configFile = ctx.fs.readUtf8File(configPath);
  } catch {
    return null;
  }
  try {
    const storedConfig = JSON.parse(configFile);
    // zod types optional properties as `T | undefined`.
    const config = schema.parse(storedConfig) as GlobalConfig;
    for (const account of Object.values(config.accounts ?? {})) {
      const { isPrimary } = account as { isPrimary?: boolean };
      if (isPrimary !== undefined) {
        delete (account as { isPrimary?: boolean }).isPrimary;
        if (isPrimary) {
          account.isDefault = true;
        }
      }
    }
    return config;
  } catch (err) {
    // Print an error and act as if the file does not exist.
    logError(
      chalkStderr.red(
        `Failed to parse global config in ${configPath} with error ${
          err as any
        }.`,
      ),
    );
    return null;
  }
}

/** Write the global config, preserving existing properties we don't understand. */
export async function modifyGlobalConfig(ctx: Context, config: GlobalConfig) {
  const configPath = globalConfigPath();
  let configFile;
  try {
    configFile = ctx.fs.readUtf8File(configPath);
    // totally fine for it not to exist
    // eslint-disable-next-line no-empty
  } catch {}
  // storedConfig may contain properties this version of the CLI doesn't understand.
  let storedConfig = {};
  if (configFile) {
    try {
      storedConfig = JSON.parse(configFile);
      schema.parse(storedConfig);
    } catch (err) {
      logError(
        chalkStderr.red(
          `Failed to parse global config in ${configPath} with error ${
            err as any
          }.`,
        ),
      );
      storedConfig = {};
    }
  }
  const newConfig: GlobalConfig = { ...storedConfig, ...config };
  await overrwriteGlobalConfig(ctx, newConfig);
}

/**
 * Read the global config, apply `update`, and write the result. Properties this
 * version of the CLI doesn't understand are preserved. When the update leaves
 * no token and no accounts, the file is deleted as on logout.
 */
export async function updateGlobalConfig(
  ctx: Context,
  update: (config: GlobalConfig | null) => GlobalConfig,
): Promise<GlobalConfig> {
  const configPath = globalConfigPath();
  let unknownProperties: Record<string, unknown> = {};
  if (ctx.fs.exists(configPath)) {
    try {
      const {
        accessToken: _accessToken,
        accounts: _accounts,
        instances: _instances,
        settings: _settings,
        ...rest
      } = JSON.parse(ctx.fs.readUtf8File(configPath));
      unknownProperties = rest;
    } catch {
      // readGlobalConfig below reports the parse error.
    }
  }
  const newConfig = update(readGlobalConfig(ctx));
  if (
    newConfig.accessToken === undefined &&
    Object.keys(newConfig.accounts ?? {}).length === 0
  ) {
    if (ctx.fs.exists(configPath)) {
      ctx.fs.unlink(configPath);
    }
    return newConfig;
  }
  await overrwriteGlobalConfig(ctx, { ...unknownProperties, ...newConfig });
  return newConfig;
}

/** Write global config, overwriting any existing settings. */
async function overrwriteGlobalConfig(ctx: Context, config: GlobalConfig) {
  const dirName = rootDirectory();
  ctx.fs.mkdir(dirName, { allowExisting: true });
  const path = globalConfigPath();
  try {
    ctx.fs.writeUtf8File(path, JSON.stringify(config, null, 2));
  } catch (err) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      errForSentry: err,
      printedMessage: chalkStderr.red(
        `Failed to write auth config to ${path} with error: ${err as any}`,
      ),
    });
  }
  logVerbose(`Saved credentials to ${formatPathForPrinting(path)}`);
}

export function formatPathForPrinting(path: string) {
  const homedir = os.homedir();
  if (process.platform === "darwin" && path.startsWith(homedir)) {
    return path.replace(homedir, "~");
  }
  return path;
}
