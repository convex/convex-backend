import path from "path";
import { logMessage } from "../../bundler/log.js";
import type { Context } from "../../bundler/context.js";
import { readProjectConfig } from "./config.js";
import { functionsDir } from "./utils/utils.js";
import {
  checkAiFilesStalenessAndLog,
  isAiFilesDisabled,
} from "./aiFiles/index.js";
import { getVersion } from "./versionApi.js";

/**
 * Fetch the latest version data, log any server nag message, and warn if
 * Convex AI files are out of date. Both checks share the one getVersion()
 * round-trip.
 */
export async function checkVersionAndAiFilesStaleness(ctx: Context) {
  const version = await getVersion();
  if (version.kind === "error") return;

  if (version.data.message) logMessage(version.data.message);

  try {
    const { configPath, projectConfig } = await readProjectConfig(ctx);
    const aiFilesConfig = projectConfig.aiFiles;
    if (isAiFilesDisabled(aiFilesConfig)) return;
    const convexDir = path.resolve(functionsDir(configPath, projectConfig));
    const projectDir = path.resolve(path.dirname(configPath));
    await checkAiFilesStalenessAndLog({
      canonicalGuidelinesHash: version.data.guidelinesHash,
      canonicalAgentSkillsSha: version.data.agentSkillsSha,
      aiFilesConfig,
      projectDir,
      convexDir,
    });
  } catch {
    // Non-fatal: skip staleness check if project config can't be resolved.
  }
}
