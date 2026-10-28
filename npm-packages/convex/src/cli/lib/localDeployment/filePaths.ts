/*
~/.cache/convex
  binaries
    0.0.1
      convex-local-backend[.exe] // convex-local-backend.exe on windows
    0.0.2
      convex-local-backend[.exe]
  dashboard
    config.json // `GlobalDashboardConfig`
    out
    // if present, output files from building the self-hosted dashboard which can
    // be served using `npx serve`
    index.html


Deployment state storage locations:

New default (project-local): .convex/local/default/
  - Used for both "local" (linked to Convex project) and "anonymous" deployments
  - One deployment per project/worktree/clone
  - This allows worktrees/clones to have isolated storage without conflicts

Legacy (home directory) - used for backward compatibility if data already exists:
  - For "local" deployments: ~/.convex/convex-backend-state/local-{team}-{project}/
  - For "anonymous" deployments: ~/.convex/anonymous-convex-backend-state/{anonymous-deployment-name}/

For anonymous deployments, the locally running dashboard's `{ port }`
is stored as `dashboard.json` next to that deployment's `config.json` (see
`ProjectDashboardConfig`), wherever the deployment lives.


~/.convex
  convex-backend-state
    local-my_team-chess
      config.json // contains `LocalDeploymentConfig`
      convex_local_storage
      convex_local_backend.sqlite3
    local-my_team-whisper
      config.json
      convex_local_storage
      convex_local_backend.sqlite3
  anonymous-convex-backend-state
    config.json // contains { uuid: <uuid> }, used to identify the anonymous user
    anonymous-chess
      config.json
      convex_local_storage
      convex_local_backend.sqlite3
*/

import path from "path";
import { cacheDir, rootDirectory } from "../utils/utils.js";
import { Context } from "../../../bundler/context.js";
import { logVerbose } from "../../../bundler/log.js";
import { recursivelyDelete } from "../fsUtils.js";
import crypto from "crypto";

/**
 * Ensure the `.convex/.gitignore` file exists with the right content.
 * This prevents local deployment state from being committed to git.
 */
export function ensureDotConvexGitignore(
  ctx: Context,
  projectDir?: string,
): void {
  const baseDir = projectDir ?? process.cwd();
  const dotConvexDir = path.join(baseDir, ".convex");
  const gitignorePath = path.join(dotConvexDir, ".gitignore");

  // Only create if .convex directory exists but .gitignore doesn't
  if (ctx.fs.exists(dotConvexDir) && !ctx.fs.exists(gitignorePath)) {
    ctx.fs.writeUtf8File(gitignorePath, "/*\n");
    logVerbose(`Created .convex/.gitignore to ignore local/`);
  }
}

// Naming is hard, but "local" refers to deployments linked to a Convex project
// and "anonymous" refers to deployments that are not linked to a Convex project
// (but in both cases they are running locally).
export type LocalDeploymentKind = "local" | "anonymous";

export function rootDeploymentStateDir(kind: LocalDeploymentKind) {
  return path.join(
    rootDirectory(),
    kind === "local"
      ? "convex-backend-state"
      : "anonymous-convex-backend-state",
  );
}

/**
 * Get the project-local state directory for a deployment.
 * Always returns `.convex/local/default/` - one deployment per project.
 */
export function projectLocalStateDir(projectDir?: string): string {
  const baseDir = projectDir ?? process.cwd();
  return path.join(baseDir, ".convex", "local", "default");
}

/**
 * Get the legacy home directory state path for a deployment.
 */
export function legacyDeploymentStateDir(
  deploymentKind: LocalDeploymentKind,
  deploymentName: string,
): string {
  return path.join(rootDeploymentStateDir(deploymentKind), deploymentName);
}

/**
 * Get the state directory for a deployment.
 *
 * Priority order:
 * 1. Project-local directory if it has data (config.json exists)
 * 2. Legacy home directory if it exists (backward compatibility)
 * 3. Project-local directory for new deployments
 *
 * This ensures that when project-local storage is in use, it takes precedence
 * over any legacy directories that might exist with the same deployment name.
 */
export function deploymentStateDir(
  ctx: Context,
  deploymentKind: LocalDeploymentKind,
  deploymentName: string,
  projectDir?: string,
): string {
  // Check if project-local has data - if so, use it
  // This takes precedence over legacy to support switching deployment types
  // within the same project directory
  const localDir = projectLocalStateDir(projectDir);
  const localConfigFile = path.join(localDir, "config.json");
  if (ctx.fs.exists(localConfigFile)) {
    logVerbose(
      `Using project-local state for deployment ${deploymentName}: ${localDir}`,
    );
    return localDir;
  }

  // Check if legacy data exists in home directory
  const legacyDir = legacyDeploymentStateDir(deploymentKind, deploymentName);
  if (ctx.fs.exists(legacyDir) && ctx.fs.stat(legacyDir).isDirectory()) {
    logVerbose(
      `Using legacy home directory state for deployment ${deploymentName}: ${legacyDir}`,
    );
    return legacyDir;
  }

  // Default to project-local for new deployments
  logVerbose(
    `Using project-local state for new deployment ${deploymentName}: ${localDir}`,
  );
  return localDir;
}

/**
 * Get the state directory for a deployment without checking for legacy data.
 * This always returns the project-local path.
 */
export function deploymentStateDirUnchecked(projectDir?: string): string {
  return projectLocalStateDir(projectDir);
}

export type LocalDeploymentConfig = {
  ports: {
    cloud: number;
    site: number;
  };
  backendVersion: string;
  adminKey: string;
  // If not present, use the default instance secret for local backends
  instanceSecret?: string;
  // The deployment name (e.g., "local-my_team-my_project" or "anonymous-chess")
  // This is stored in the config for project-local storage where the directory
  // name is always "default" rather than the deployment name.
  deploymentName?: string;
  // Numeric id of the cloud project this local deployment is registered under.
  // Used to detect when the cloud project has changed since the last time it
  // was used (e.g. through `npx convex deployment select`).
  cloudProjectId: number | undefined;
};

/**
 * Load deployment config from a specific directory.
 * This is used when we already know the directory path.
 */
export function loadDeploymentConfigFromDir(
  ctx: Context,
  dir: string,
): LocalDeploymentConfig | null {
  const configFile = path.join(dir, "config.json");
  if (!ctx.fs.exists(configFile)) {
    return null;
  }
  const content = ctx.fs.readUtf8File(configFile);
  try {
    return JSON.parse(content);
  } catch (e) {
    logVerbose(
      `Failed to parse local deployment config at ${dir}: ${e as any}`,
    );
    return null;
  }
}

/**
 * Load the project-local deployment config.
 * This returns the config from `.convex/local/default/` if it exists.
 * Returns both the config and the deployment name stored in it.
 */
export function loadProjectLocalConfig(
  ctx: Context,
  projectDir?: string,
): { deploymentName: string; config: LocalDeploymentConfig } | null {
  const localDir = projectLocalStateDir(projectDir);
  const config = loadDeploymentConfigFromDir(ctx, localDir);
  if (config !== null && config.deploymentName) {
    logVerbose(
      `Found project-local deployment config for ${config.deploymentName}`,
    );
    return { deploymentName: config.deploymentName, config };
  }
  return null;
}

/**
 * Load deployment config for a deployment.
 *
 * Priority order (matching deploymentStateDir):
 * 1. Project-local directory if it has a matching config
 * 2. Legacy home directory
 */
export function loadDeploymentConfig(
  ctx: Context,
  deploymentKind: LocalDeploymentKind,
  deploymentName: string,
  projectDir?: string,
): LocalDeploymentConfig | null {
  // Check project-local location first - matches deploymentStateDir priority
  const localDir = projectLocalStateDir(projectDir);
  const localConfig = loadDeploymentConfigFromDir(ctx, localDir);
  if (localConfig !== null) {
    // Only use if config matches the requested deployment name
    // (project-local can hold different deployments at different times)
    if (
      !localConfig.deploymentName ||
      localConfig.deploymentName === deploymentName
    ) {
      logVerbose(
        `Found deployment config in project-local location for ${deploymentName}`,
      );
      return localConfig;
    }
    logVerbose(
      `Project-local config is for ${localConfig.deploymentName}, not ${deploymentName}`,
    );
  }

  // Check legacy location
  const legacyDir = legacyDeploymentStateDir(deploymentKind, deploymentName);
  const legacyConfig = loadDeploymentConfigFromDir(ctx, legacyDir);
  if (legacyConfig !== null) {
    logVerbose(
      `Found deployment config in legacy location for ${deploymentName}`,
    );
    return legacyConfig;
  }

  return null;
}

/**
 * Save deployment config.
 *
 * If data already exists in the legacy home directory, continue using that
 * location. Otherwise, use the project-local directory. The deployment name
 * is always stored in the config for project-local storage.
 */
export function saveDeploymentConfig(
  ctx: Context,
  deploymentKind: LocalDeploymentKind,
  deploymentName: string,
  config: LocalDeploymentConfig,
  projectDir?: string,
) {
  const dir = deploymentStateDir(
    ctx,
    deploymentKind,
    deploymentName,
    projectDir,
  );
  const configFile = path.join(dir, "config.json");
  if (!ctx.fs.exists(dir)) {
    ctx.fs.mkdir(dir, { recursive: true });
  }
  // Ensure .gitignore exists to prevent local state from being committed
  ensureDotConvexGitignore(ctx, projectDir);
  // Always include the deployment name in the config for project-local storage
  const configWithName = { ...config, deploymentName };
  ctx.fs.writeUtf8File(configFile, JSON.stringify(configWithName));
}

export function binariesDir() {
  return path.join(cacheDir(), "binaries");
}

export function dashboardZip() {
  return path.join(dashboardDir(), "dashboard.zip");
}

export function versionedBinaryDir(version: string) {
  return path.join(binariesDir(), version);
}

export function executablePath(version: string) {
  return path.join(versionedBinaryDir(version), executableName());
}

export function executableName() {
  const ext = process.platform === "win32" ? ".exe" : "";
  return `convex-local-backend${ext}`;
}

export function dashboardDir() {
  return path.join(cacheDir(), "dashboard");
}

export async function resetDashboardDir(ctx: Context) {
  const dir = dashboardDir();
  if (ctx.fs.exists(dir)) {
    await recursivelyDelete(ctx, dir);
  }
  ctx.fs.mkdir(dir, { recursive: true });
}

export function dashboardOutDir() {
  return path.join(dashboardDir(), "out");
}

/**
 * Global dashboard config in ~/.cache/convex/dashboard/config.json.
 */
export type GlobalDashboardConfig = {
  version: string;

  // Previous versions of the Convex CLI would also store
  // fields here for ports. They are now in ProjectDashboardConfig.
};

export function loadGlobalDashboardConfig(
  ctx: Context,
): GlobalDashboardConfig | null {
  const configFile = path.join(dashboardDir(), "config.json");
  if (!ctx.fs.exists(configFile)) {
    return null;
  }
  const content = ctx.fs.readUtf8File(configFile);
  try {
    return JSON.parse(content);
  } catch (e) {
    logVerbose(`Failed to parse dashboard config: ${e as any}`);
    return null;
  }
}

export function saveGlobalDashboardConfig(
  ctx: Context,
  config: GlobalDashboardConfig,
) {
  const configFile = path.join(dashboardDir(), "config.json");
  if (!ctx.fs.exists(dashboardDir())) {
    ctx.fs.mkdir(dashboardDir(), { recursive: true });
  }
  ctx.fs.writeUtf8File(configFile, JSON.stringify(config));
}

/**
 * Project-scoped dashboard config (for anonymous dev) in
 * project/.convex/local/default/dashboard.json
 * (or legacyDeploymentStateDir/dashboard.json)
 */
export type ProjectDashboardConfig = {
  port: number;
};

function dashboardConfigPath(ctx: Context, deploymentName: string) {
  return path.join(
    deploymentStateDir(ctx, "anonymous", deploymentName),
    "dashboard.json",
  );
}

export function loadProjectDashboardConfig(
  ctx: Context,
  deploymentName: string,
): ProjectDashboardConfig | null {
  const configFile = dashboardConfigPath(ctx, deploymentName);
  if (!ctx.fs.exists(configFile)) {
    return null;
  }
  const content = ctx.fs.readUtf8File(configFile);
  try {
    return JSON.parse(content);
  } catch (e) {
    logVerbose(`Failed to parse project dashboard config: ${e as any}`);
    return null;
  }
}

export function saveProjectDashboardConfig(
  ctx: Context,
  deploymentName: string,
  config: ProjectDashboardConfig,
) {
  const configPath = dashboardConfigPath(ctx, deploymentName);
  const directoryPath = path.dirname(configPath);
  if (!ctx.fs.exists(directoryPath)) {
    ctx.fs.mkdir(directoryPath, { recursive: true });
  }
  ctx.fs.writeUtf8File(configPath, JSON.stringify(config));
}

export function loadUuidForAnonymousUser(ctx: Context) {
  const configFile = path.join(
    rootDeploymentStateDir("anonymous"),
    "config.json",
  );
  if (!ctx.fs.exists(configFile)) {
    return null;
  }
  const content = ctx.fs.readUtf8File(configFile);
  try {
    const config = JSON.parse(content);
    return config.uuid ?? null;
  } catch (e) {
    logVerbose(`Failed to parse uuid for anonymous user: ${e as any}`);
    return null;
  }
}

export function ensureUuidForAnonymousUser(ctx: Context) {
  const uuid = loadUuidForAnonymousUser(ctx);
  if (uuid) {
    return uuid;
  }
  const newUuid = crypto.randomUUID();
  const anonymousDir = rootDeploymentStateDir("anonymous");
  if (!ctx.fs.exists(anonymousDir)) {
    ctx.fs.mkdir(anonymousDir, { recursive: true });
  }
  ctx.fs.writeUtf8File(
    path.join(anonymousDir, "config.json"),
    JSON.stringify({ uuid: newUuid }),
  );
  return newUuid;
}
