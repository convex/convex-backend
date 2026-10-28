import * as Sentry from "@sentry/node";
import path from "path";
import { Context } from "../../../bundler/context.js";
// eslint-disable-next-line no-restricted-imports
import { promises as fs } from "fs";
import { chalkStderr } from "chalk";
import { logMessage } from "../../../bundler/log.js";
import { promptYesNo } from "../utils/prompts.js";
import { fetchAgentSkillsCatalog } from "../versionApi.js";
import { type AiFilesPaths, aiDirForConvexDir } from "./paths.js";
import {
  installGuidelinesFile,
  hasGuidelinesInstalled,
} from "./guidelinesmd.js";
import {
  attemptReadAiState,
  readAiStateOrDefault,
  writeAiState,
  hasAiState,
} from "./state.js";
import { type AiFilesProjectConfig } from "../config.js";
import { exhaustiveCheck, isInInteractiveTerminal } from "./utils.js";
import {
  hasAgentsMdInstalled,
  applyAgentsMdSection,
  attemptToRemoveAgentsMdSection,
} from "./agentsmd.js";
import {
  hasClaudeMdInstalled,
  applyClaudeMdSection,
  attemptToRemoveClaudeMdSection,
} from "./claudemd.js";
import { installSkills, removeInstalledSkills } from "./skills.js";
import { removeLegacyCursorRulesFile as removeLegacyCursorRules } from "./cursorrules.js";

async function hasExistingAiFilesArtifacts({
  projectDir,
  convexDir,
}: AiFilesPaths): Promise<boolean> {
  return (
    (await hasGuidelinesInstalled(convexDir)) ||
    (await hasAgentsMdInstalled(projectDir)) ||
    (await hasClaudeMdInstalled(projectDir))
  );
}

type InstallAiFilesResult =
  | { kind: "success" }
  | { kind: "error"; message: string };

/**
 * Install or refresh all Convex AI files.
 *
 * Reads the existing state if present, or starts from a blank one for a
 * fresh install.
 *
 * Writes everything it can even when the guidelines download or the skills
 * install fails, and returns an error if either of them failed.
 */
export async function installAiFiles({
  projectDir,
  convexDir,
  aiFilesConfig,
}: AiFilesPaths & {
  aiFilesConfig?: AiFilesProjectConfig | undefined;
}): Promise<InstallAiFilesResult> {
  const convexDirName = path.relative(projectDir, convexDir);
  const state = await readAiStateOrDefault(convexDir);

  const guidelinesInstalled = await installGuidelinesFile({ convexDir, state });
  await applyAgentsMdSection({ projectDir, state, convexDirName });
  await applyClaudeMdSection({ projectDir, state, convexDirName });
  const skillsInstalled = await installSkills({
    projectDir,
    state,
    aiFilesConfig,
  });
  await removeLegacyCursorRules(projectDir);
  await writeAiState({ state, convexDir });

  if (!guidelinesInstalled || !skillsInstalled) {
    return {
      kind: "error",
      message:
        "Some Convex AI files could not be installed. See the messages above.",
    };
  }
  return { kind: "success" };
}

async function attemptToInstallAiFiles(
  opts: Parameters<typeof installAiFiles>[0],
): Promise<void> {
  try {
    await installAiFiles(opts);
  } catch (error) {
    Sentry.captureException(error);
  }
}

type AiFilesStalenessStatus = "not-installed" | "stale" | "silent";

export function isAiFilesDisabled(
  aiFilesConfig: AiFilesProjectConfig | undefined,
): boolean {
  if (aiFilesConfig?.enabled !== undefined)
    return aiFilesConfig.enabled === false;
  return aiFilesConfig?.disableStalenessMessage === true;
}

async function determineAiFilesStaleness({
  canonicalGuidelinesHash,
  canonicalAgentSkillsSha,
  aiFilesConfig,
  projectDir,
  convexDir,
}: {
  canonicalGuidelinesHash: string | null;
  canonicalAgentSkillsSha: string | null;
  aiFilesConfig?: AiFilesProjectConfig | undefined;
} & AiFilesPaths): Promise<AiFilesStalenessStatus> {
  if (isAiFilesDisabled(aiFilesConfig)) return "silent";

  const result = await attemptReadAiState(convexDir);

  if (result.kind === "no-file" || result.kind === "parse-error") {
    const hasArtifacts = await hasExistingAiFilesArtifacts({
      projectDir,
      convexDir,
    });
    return hasArtifacts ? "silent" : "not-installed";
  }

  if (result.kind === "ok") {
    const { state } = result;

    if (canonicalGuidelinesHash === null && canonicalAgentSkillsSha === null)
      return "silent";

    const guidelinesStale =
      canonicalGuidelinesHash !== null &&
      state.guidelinesHash !== null &&
      state.guidelinesHash !== canonicalGuidelinesHash;

    const skillsStale =
      canonicalAgentSkillsSha !== null &&
      state.agentSkillsSha !== null &&
      state.agentSkillsSha !== canonicalAgentSkillsSha;

    return guidelinesStale || skillsStale ? "stale" : "silent";
  }

  return exhaustiveCheck(result);
}

/**
 * Check whether the Convex AI files are out of date and log a nag message
 * if so.
 */
export async function checkAiFilesStalenessAndLog(
  opts: {
    canonicalGuidelinesHash: string | null;
    canonicalAgentSkillsSha: string | null;
    aiFilesConfig?: AiFilesProjectConfig | undefined;
  } & AiFilesPaths,
): Promise<void> {
  const status = await determineAiFilesStaleness(opts);

  if (status === "not-installed") {
    logMessage(
      chalkStderr.yellow(
        `Convex AI files are not installed. Run ${chalkStderr.bold(`npx convex ai-files install`)} to get started or ${chalkStderr.bold(`npx convex ai-files disable`)} to hide this message.`,
      ),
    );
    return;
  }

  if (status === "stale") {
    logMessage(
      chalkStderr.yellow(
        `Your Convex AI files are out of date. Run ${chalkStderr.bold(`npx convex ai-files update`)} to get the latest.`,
      ),
    );
    return;
  }

  if (status === "silent") return;

  exhaustiveCheck(status);
}

/**
 * Installs AI files and returns the aiFiles config to write.
 */
export async function enableAiFiles({
  projectDir,
  convexDir,
  aiFilesConfig,
}: AiFilesPaths & {
  aiFilesConfig?: AiFilesProjectConfig | undefined;
}): Promise<AiFilesProjectConfig> {
  await installAiFiles({ projectDir, convexDir, aiFilesConfig });
  // Deleting the deprecated disableStalenessMessage key
  const { disableStalenessMessage: _, ...rest } = aiFilesConfig ?? {};
  return { ...rest, enabled: true };
}

/**
 * Returns the aiFiles config to write when disabling AI files.
 */
export function disableAiFiles(
  aiFilesConfig?: AiFilesProjectConfig | undefined,
): AiFilesProjectConfig {
  // Deleting the deprecated disableStalenessMessage key
  const { disableStalenessMessage: _, ...rest } = aiFilesConfig ?? {};
  return { ...rest, enabled: false };
}

export type RemoveAiFilesResult =
  | { kind: "success" }
  | { kind: "error"; message: string };

/**
 * Remove all Convex AI files from the project.
 * Called by `npx convex ai-files remove`.
 */
export async function removeAiFiles({
  projectDir,
  convexDir,
}: AiFilesPaths): Promise<RemoveAiFilesResult> {
  const agentSkillsCatalog = await fetchAgentSkillsCatalog();
  if (agentSkillsCatalog.kind === "error") {
    return {
      kind: "error",
      message:
        "Could not fetch canonical agent skills from version.convex.dev. Aborting `convex ai-files remove`.",
    };
  }

  const removals = [
    await attemptToRemoveAgentsMdSection(projectDir),
    await attemptToRemoveClaudeMdSection(projectDir),
    (await removeInstalledSkills({
      projectDir,
      skillNames: agentSkillsCatalog.data.skills.map(
        ({ skillName }) => skillName,
      ),
    })) === "removed",
    await removeLegacyCursorRules(projectDir),
    await attemptToDeleteAiDir({ projectDir, convexDir }),
  ];

  if (removals.some(Boolean)) logMessage("Convex AI files removed.");
  else logMessage("No Convex AI files found — nothing to remove.");
  return { kind: "success" };
}

async function attemptToDeleteAiDir({
  projectDir,
  convexDir,
}: AiFilesPaths): Promise<boolean> {
  const aiDir = aiDirForConvexDir(convexDir);
  const relPath = path.relative(projectDir, aiDir);
  try {
    await fs.rm(aiDir, { recursive: true });
    logMessage(`${chalkStderr.green("✔")} Deleted ${relPath}/`);
    return true;
  } catch (error) {
    if ((error as { code?: string }).code === "ENOENT") return false;
    Sentry.captureException(error);
    logMessage(
      chalkStderr.yellow(`Could not delete ${relPath}/. Remove it manually.`),
    );
    return false;
  }
}

async function hasAiFilesBeenInstalledBefore({
  projectDir,
  convexDir,
  aiFilesConfig,
}: AiFilesPaths & {
  aiFilesConfig?: AiFilesProjectConfig | undefined;
}): Promise<boolean> {
  if (isAiFilesDisabled(aiFilesConfig)) return false;
  return (
    (await hasAiState(convexDir)) ||
    (await hasExistingAiFilesArtifacts({ projectDir, convexDir }))
  );
}

export async function attemptSetupAiFiles({
  ctx,
  convexDir,
  projectDir,
  aiFilesConfig,
}: {
  ctx: Context;
  aiFilesConfig?: AiFilesProjectConfig | undefined;
} & AiFilesPaths): Promise<void> {
  if (!isInInteractiveTerminal()) return;
  if (isAiFilesDisabled(aiFilesConfig)) return;

  if (
    await hasAiFilesBeenInstalledBefore({
      projectDir,
      convexDir,
      aiFilesConfig,
    })
  ) {
    await attemptToInstallAiFiles({ projectDir, convexDir, aiFilesConfig });
    return;
  }

  const shouldInstall = await promptYesNo(ctx, {
    message: "Set up Convex AI files? (guidelines, AGENTS.md, agent skills)",
    default: true,
  });

  if (shouldInstall)
    await attemptToInstallAiFiles({ projectDir, convexDir, aiFilesConfig });
}
