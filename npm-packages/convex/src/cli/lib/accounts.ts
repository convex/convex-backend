import path from "path";
import { isDeepStrictEqual } from "util";
import { randomUUID } from "crypto";
import { TempPath } from "../../bundler/fs.js";
import * as dotenv from "dotenv";
import { z } from "zod";
import { BigBrainAuth, Context } from "../../bundler/context.js";
import { logFinishedStep, logWarning } from "../../bundler/log.js";
import {
  ensureNotRunningUnderSudo,
  globalConfigPath,
  readGlobalConfig,
  restrictConfigPermissions,
} from "./utils/globalConfig.js";
import { ENV_VAR_FILE_PATH, rootDirectory } from "./utils/utils.js";

import { CONVEX_ACCOUNT_ENV_VAR } from "./envvars.js";

export type AccountSelection = {
  name: string;
  source: string;
};

const settingsSchema = z
  .object({
    defaultAccount: z.string().optional(),
    directories: z.record(z.string()).optional(),
  })
  .passthrough();

const credentialsSchema = z
  .object({
    accessToken: z.string().min(1),
    memberId: z.string().optional(),
    email: z.string().optional(),
  })
  .passthrough();

export type AccountCredentials = z.infer<typeof credentialsSchema>;

let warnedAboutUnreadableSettings = false;

export async function readAccountSettings(
  ctx: Context,
): Promise<z.infer<typeof settingsSchema>> {
  if (!ctx.fs.exists(globalConfigPath())) return {};
  let stored: unknown;
  try {
    const contents = ctx.fs.readUtf8File(globalConfigPath());
    restrictConfigPermissions(ctx, globalConfigPath());
    stored = JSON.parse(contents);
  } catch (err: any) {
    if (!(err instanceof SyntaxError)) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "invalid filesystem data",
        printedMessage: `Unable to read account settings from ${globalConfigPath()}: ${err}`,
      });
    }
    stored = null;
  }
  if (typeof stored !== "object" || stored === null || Array.isArray(stored)) {
    // Older CLI versions write this file in place, so a read can observe a
    // partial write. Logging in replaces the file.
    if (!warnedAboutUnreadableSettings) {
      logWarning(
        `Ignoring ${globalConfigPath()} because it does not contain a JSON object. Logging in or changing account settings replaces it.`,
      );
      warnedAboutUnreadableSettings = true;
    }
    return {};
  }
  const parsed = settingsSchema.safeParse(stored);
  if (parsed.success) return parsed.data;
  const key = parsed.error.issues[0]?.path.join(".") || "its contents";
  return await ctx.crash({
    exitCode: 1,
    errorType: "invalid filesystem data",
    printedMessage: `Unable to read account settings from ${globalConfigPath()}: ${key} is invalid. defaultAccount must be an account name and directories must map directory paths to account names.`,
  });
}

function writeAccountSettings(ctx: Context, settings: Record<string, unknown>) {
  if (Object.keys(settings).length === 0) {
    if (ctx.fs.exists(globalConfigPath())) ctx.fs.unlink(globalConfigPath());
    return;
  }
  const temporary = `${globalConfigPath()}.${randomUUID()}.tmp` as TempPath;
  try {
    ctx.fs.writeUtf8File(temporary, JSON.stringify(settings, null, 2), 0o600);
    ctx.fs.swapTmpFile(temporary, globalConfigPath());
  } finally {
    if (ctx.fs.exists(temporary)) ctx.fs.unlink(temporary);
  }
}

export async function updateAccountSettings<T>(
  ctx: Context,
  update: (settings: z.infer<typeof settingsSchema>) => T | Promise<T>,
  afterWrite?: () => void,
): Promise<T> {
  // Replacing the settings file as root would change its owner.
  await ensureNotRunningUnderSudo(ctx);
  const lockPath = `${globalConfigPath()}.lock`;
  ctx.fs.mkdir(rootDirectory(), { allowExisting: true });
  const deadline = Date.now() + 5000;
  for (;;) {
    try {
      ctx.fs.mkdir(lockPath);
      break;
    } catch (err: any) {
      // Only the process that creates the directory owns the update lock.
      // eslint-disable-next-line no-restricted-syntax
      if (err.code !== "EEXIST") throw err;
      if (Date.now() >= deadline) {
        return await ctx.crash({
          exitCode: 1,
          errorType: "fatal",
          printedMessage: `Another command is updating Convex accounts. Retry after it finishes. If it was interrupted, remove ${lockPath}.`,
        });
      }
      await new Promise((resolve) => setTimeout(resolve, 25));
    }
  }
  let released = false;
  const release = () => {
    if (released) return;
    ctx.fs.rmdir(lockPath);
    released = true;
  };
  const cleanup = ctx.registerCleanup(async () => release());
  try {
    const settings = await readAccountSettings(ctx);
    const result = await update(settings);
    writeAccountSettings(ctx, settings);
    // Credential deletion follows persistence while sharing the lock with renewals.
    afterWrite?.();
    return result;
  } finally {
    ctx.removeCleanup(cleanup);
    release();
  }
}

export function accountConfigPath(name: string): string {
  return path.join(
    rootDirectory(),
    "accounts",
    `${encodeURIComponent(name)}.json`,
  );
}

function isValidAccountName(name: string): boolean {
  return /^[a-z0-9][a-z0-9_-]*$/.test(name);
}

export async function validateAccountName(
  ctx: Context,
  name: string,
  source?: string,
): Promise<void> {
  if (!isValidAccountName(name)) {
    await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem or env vars",
      printedMessage: `${source === undefined ? "" : `Invalid account name "${name}" from ${source}. `}Account names must start with a lowercase letter or number and contain only lowercase letters, numbers, hyphens, and underscores.`,
    });
  }
}

const authEnvFiles = new WeakMap<Context, string>();

/**
 * The `--env-file` a command's credentials were resolved with. A login started
 * later in the command selects its account from the same file, whichever
 * credentials the command started with.
 */
export function rememberAuthEnvFile(ctx: Context, envFile: string | undefined) {
  if (envFile === undefined) authEnvFiles.delete(ctx);
  else authEnvFiles.set(ctx, envFile);
}

export function authEnvFile(ctx: Context): string | undefined {
  return authEnvFiles.get(ctx);
}

export async function getAccountSelection(
  ctx: Context,
  options: { name?: string | undefined; envFile?: string | undefined } = {},
): Promise<AccountSelection | null> {
  let name = options.name;
  let source = "--account";
  // An empty CONVEX_ACCOUNT is unset, like the other CONVEX_* variables.
  if (name === undefined && process.env[CONVEX_ACCOUNT_ENV_VAR]) {
    name = process.env[CONVEX_ACCOUNT_ENV_VAR];
    source = `${CONVEX_ACCOUNT_ENV_VAR} environment variable`;
  }
  if (name === undefined) {
    const files = options.envFile
      ? [options.envFile]
      : [ENV_VAR_FILE_PATH, ".env"];
    for (const file of files) {
      if (!ctx.fs.exists(file)) continue;
      name =
        dotenv.parse(ctx.fs.readUtf8File(file))[CONVEX_ACCOUNT_ENV_VAR] ||
        undefined;
      if (name !== undefined) {
        source = `${CONVEX_ACCOUNT_ENV_VAR} in ${file}`;
        break;
      }
    }
  }
  if (name === undefined) {
    const config = await readAccountSettings(ctx);
    const binding = findAccountDirectory(
      config.directories ?? {},
      process.cwd(),
    );
    name = binding?.name ?? config.defaultAccount;
    source = binding
      ? `directory selection for ${binding.directory}`
      : `default account in ${globalConfigPath()}`;
  }
  if (name === undefined) return null;
  await validateAccountName(ctx, name, source);
  return { name, source };
}

export async function readAccountAuth(
  ctx: Context,
  account: AccountSelection,
  allowMissing = false,
): Promise<Extract<BigBrainAuth, { kind: "accessToken" }> | null> {
  const credentials = await readAccountCredentials(ctx, account, allowMissing);
  return credentials === null
    ? null
    : {
        kind: "accessToken",
        accessToken: credentials.accessToken,
        header: `Bearer ${credentials.accessToken}`,
        account,
      };
}

export function accountNotLoggedInMessage(account: AccountSelection): string {
  return `Account "${account.name}" selected by ${account.source} is not logged in. Run \`npx convex login --account ${account.name}\`.`;
}

/**
 * Account credential files are written with mode 0600, but a copied or
 * restored file can be readable by other users. Like ~/.convex/config.json,
 * remove group and other access from the file and the accounts directory
 * whenever a credential is read.
 */
function restrictAccountPermissions(ctx: Context, configPath: string) {
  restrictConfigPermissions(ctx, path.dirname(configPath));
  restrictConfigPermissions(ctx, configPath);
}

/** Credentials saved for a valid account name, or why there are none to use. */
export function loadAccountCredentials(
  ctx: Context,
  name: string,
): AccountCredentials | "missing" | "unreadable" {
  const configPath = accountConfigPath(name);
  if (!ctx.fs.exists(configPath)) return "missing";
  restrictAccountPermissions(ctx, configPath);
  try {
    return credentialsSchema.parse(JSON.parse(ctx.fs.readUtf8File(configPath)));
  } catch {
    return "unreadable";
  }
}

export async function readAccountCredentials(
  ctx: Context,
  account: AccountSelection,
  allowMissing = false,
): Promise<AccountCredentials | null> {
  await validateAccountName(ctx, account.name, account.source);
  const credentials = loadAccountCredentials(ctx, account.name);
  if (credentials === "missing") {
    if (allowMissing) return null;
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: accountNotLoggedInMessage(account),
    });
  }
  if (credentials === "unreadable") {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Unable to read credentials for account "${account.name}" from ${accountConfigPath(account.name)}. Run \`npx convex logout --account ${account.name}\` followed by \`npx convex login --account ${account.name}\` to replace them.`,
    });
  }
  return credentials;
}

export function readAccountToken(ctx: Context, name: string): string | null {
  try {
    const configPath = accountConfigPath(name);
    const contents = ctx.fs.readUtf8File(configPath);
    restrictAccountPermissions(ctx, configPath);
    const token = JSON.parse(contents).accessToken;
    return typeof token === "string" && token.length > 0 ? token : null;
  } catch {
    return null;
  }
}

/** Returns why the credentials could not be saved, or null once they are. */
function writeCredentialsFile(
  ctx: Context,
  name: string,
  credentials: AccountCredentials,
): string | null {
  const configPath = accountConfigPath(name);
  const directory = path.dirname(configPath);
  const temporary = `${configPath}.${randomUUID()}.tmp` as TempPath;
  try {
    ctx.fs.mkdir(directory, { allowExisting: true });
    // The home directory's own layout is the user's choice, so only the
    // credential store inside it has to be a real directory.
    if (!ctx.fs.lstat(directory).isDirectory()) {
      return `Cannot save account credentials through a symlink or non-directory: ${directory}.`;
    }
    // mkdir creates the directory with the umask's mode, usually 0755.
    restrictConfigPermissions(ctx, directory);
    ctx.fs.writeUtf8File(
      temporary,
      JSON.stringify(credentials, null, 2),
      0o600,
    );
    ctx.fs.swapTmpFile(temporary, configPath);
  } catch (err: any) {
    return `Failed to save account credentials to ${configPath}: ${err}`;
  } finally {
    if (ctx.fs.exists(temporary)) ctx.fs.unlink(temporary);
  }
  return null;
}

export async function writeAccountCredentials(
  ctx: Context,
  name: string,
  accessToken: string,
  metadata: Omit<AccountCredentials, "accessToken"> = {},
  expected?: AccountCredentials | null,
  onUnsaved?: () => Promise<void>,
): Promise<void> {
  await validateAccountName(ctx, name);
  let saved = false;
  let reported = false;
  // Every exit that leaves the token unsaved reports it once, including a
  // crash while waiting for or holding the settings lock.
  const reportUnsaved = async () => {
    if (saved || reported) return;
    reported = true;
    await onUnsaved?.();
  };
  const cleanup = ctx.registerCleanup(reportUnsaved);
  let failure: string | null;
  try {
    failure = await updateAccountSettings(ctx, (settings) => {
      if (expected !== undefined) {
        const current = loadAccountCredentials(ctx, name);
        if (
          !isDeepStrictEqual(current === "missing" ? null : current, expected)
        ) {
          return `Account "${name}" changed during login. Retry the login.`;
        }
      }
      const failure = writeCredentialsFile(ctx, name, {
        ...metadata,
        accessToken,
      });
      saved = failure === null;
      if (saved && settings.defaultAccount === name)
        settings.accessToken = accessToken;
      return failure;
    });
  } catch (err: any) {
    if (!saved) await reportUnsaved();
    // eslint-disable-next-line no-restricted-syntax
    throw err;
  } finally {
    ctx.removeCleanup(cleanup);
  }
  if (failure !== null) {
    // Revocation runs here, after the lock is released for other commands.
    await reportUnsaved();
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: failure,
    });
  }
}

export async function legacyAuthForFirstAccount(
  ctx: Context,
  account: AccountSelection,
): Promise<Extract<BigBrainAuth, { kind: "accessToken" }> | null> {
  const settings = await readAccountSettings(ctx);
  if (
    settings.defaultAccount !== undefined ||
    savedAccountNames(ctx).length > 0
  ) {
    return null;
  }
  const config = readGlobalConfig(ctx);
  return config === null
    ? null
    : {
        kind: "accessToken",
        accessToken: config.accessToken,
        header: `Bearer ${config.accessToken}`,
        account,
      };
}

export async function initializeDefaultAccount(
  ctx: Context,
  account: AccountSelection,
  namingLegacyAccount: boolean,
  select = true,
) {
  const defaultAccount = await updateAccountSettings(ctx, async (settings) => {
    if (settings.defaultAccount !== undefined) return undefined;
    const credentials = await readAccountCredentials(ctx, account);
    const accessToken = credentials!.accessToken;
    const legacyToken = settings.accessToken;
    if (
      !select &&
      (savedAccountNames(ctx).some((name) => name !== account.name) ||
        (!namingLegacyAccount &&
          typeof legacyToken === "string" &&
          legacyToken.length > 0 &&
          legacyToken !== accessToken))
    )
      return undefined;
    if (
      !namingLegacyAccount &&
      typeof legacyToken === "string" &&
      legacyToken.length > 0 &&
      legacyToken !== accessToken
    ) {
      // An explicitly new login keeps the previous account as the default for
      // directories that haven't selected an account.
      let legacyName = "default";
      for (
        let suffix = 2;
        ctx.fs.exists(accountConfigPath(legacyName));
        suffix++
      ) {
        legacyName = `default-${suffix}`;
      }
      const failure = writeCredentialsFile(ctx, legacyName, {
        accessToken: legacyToken,
      });
      if (failure !== null) {
        return await ctx.crash({
          exitCode: 1,
          errorType: "invalid filesystem data",
          printedMessage: failure,
        });
      }
      settings.defaultAccount = legacyName;
    } else {
      settings.defaultAccount = account.name;
      settings.accessToken = accessToken;
    }

    return settings.defaultAccount;
  });
  if (defaultAccount !== undefined)
    logFinishedStep(`Default account: ${defaultAccount}`);
}

export function findAccountDirectory(
  directories: Record<string, string>,
  cwd: string,
): { directory: string; name: string } | null {
  let directory = path.resolve(cwd);
  for (;;) {
    if (Object.hasOwn(directories, directory)) {
      return { directory, name: directories[directory] };
    }
    const parent = path.dirname(directory);
    if (parent === directory) return null;
    directory = parent;
  }
}

export function savedAccountNames(ctx: Context): string[] {
  const directory = path.join(rootDirectory(), "accounts");
  if (!ctx.fs.exists(directory)) return [];
  return (
    ctx.fs
      .listDir(directory)
      .filter((entry) => entry.name.endsWith(".json"))
      .map((entry) => entry.name.slice(0, -5))
      // Other files in the directory, such as editor backups, are not accounts.
      .filter(isValidAccountName)
      .sort()
  );
}

export async function writeAccountDirectory(ctx: Context, name: string) {
  await validateAccountName(ctx, name);
  await updateAccountSettings(ctx, (settings) => {
    settings.directories = { ...settings.directories, [process.cwd()]: name };
  });
  logFinishedStep(
    `Saved directory selection: ${name} (applies to this directory and its subdirectories).`,
  );
  const selected = await getAccountSelection(ctx);
  if (selected !== null && selected.name !== name) {
    logWarning(
      `Account "${selected.name}" selected by ${selected.source} overrides this directory selection. Remove that override to use account "${name}".`,
    );
  }
}
