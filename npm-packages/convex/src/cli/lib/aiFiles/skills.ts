import child_process from "child_process";
import path from "path";
// eslint-disable-next-line no-restricted-imports
import { promises as fs } from "fs";
import { chalkStderr } from "chalk";
import { logMessage } from "../../../bundler/log.js";
import { getVersion, fetchAgentSkillsSha } from "../versionApi.js";
import { type AiFilesState } from "./state.js";
import { exhaustiveCheck } from "./utils.js";

import { type AiFilesProjectConfig } from "../config.js";

/**
 * Resolve the configured agent list, falling back to defaults.
 */
function configuredSkillAgents(
  aiFilesConfig?: AiFilesProjectConfig | undefined,
): string[] {
  // We default to the two most popular agents for now, "codex" installs to `.agents` which also
  // covers cursor and many other tools. See: https://github.com/vercel-labs/skills?tab=readme-ov-file#supported-agents
  const defaultAgents = ["claude-code", "codex"];
  return aiFilesConfig?.skills?.agents ?? defaultAgents;
}

/**
 * Runs `npx skills add convex/agent-skills --yes --copy` in the given directory.
 * Returns true on success, false if the process fails or cannot be started.
 */
function runSkillsAdd(cwd: string, agents: string[]): Promise<boolean> {
  const args = ["add", "convex/agent-skills", "--yes", "--copy"];
  for (const agent of agents) {
    args.push("--agent", agent);
  }
  return runSkillsCommand(cwd, args).then(({ ok }) => ok);
}

/**
 * Runs `npx skills remove <name...> --yes` to surgically remove only the
 * Convex-managed skills, leaving any skills from other sources intact.
 */
function runSkillsRemove({
  cwd,
  skillNames,
}: {
  cwd: string;
  skillNames: string[];
}): Promise<boolean> {
  return runSkillsCommand(cwd, ["remove", ...skillNames, "--yes"]).then(
    ({ ok }) => ok,
  );
}

/**
 * This function exists so we have a way to disable skills installs without pushing a new
 * version of the convex CLI
 */
async function shouldRunSkillsCli(): Promise<boolean> {
  const versionData = await getVersion();

  if (versionData.kind === "error") return true;

  if (versionData.kind === "ok") {
    if (versionData.data.disableSkillsCli) {
      const message =
        versionData.data.disableSkillsCliMessage ??
        "Agent skills are temporarily disabled.";
      logMessage(chalkStderr.yellow(message));
      return false;
    }
    return true;
  }

  return exhaustiveCheck(versionData);
}

/**
 * Remove the skills-lock.json file if it only contains skills that we
 * are removing. The `npx skills remove` command leaves the lockfile behind
 * even when it's logically empty.
 */
async function removeSkillsLockIfEmpty({
  projectDir,
  removedSkillNames,
}: {
  projectDir: string;
  removedSkillNames: string[];
}): Promise<boolean> {
  const lockPath = path.join(projectDir, "skills-lock.json");
  try {
    const content = await fs.readFile(lockPath, "utf8");
    const lock = JSON.parse(content);

    if (
      !lock ||
      typeof lock !== "object" ||
      !lock.skills ||
      typeof lock.skills !== "object"
    ) {
      return false;
    }

    const remainingSkills = Object.keys(lock.skills).filter(
      (name) => !removedSkillNames.includes(name),
    );

    if (remainingSkills.length === 0) {
      await fs.unlink(lockPath);
      return true;
    }
    return false;
  } catch {
    return false;
  }
}

/**
 * Install Convex agent skills and record the SHA into the state.
 * Handles the kill-switch check and all logging internally.
 *
 * Returns false if the skills CLI failed. Skipping skills because no agents
 * are configured or the kill switch is on counts as success.
 */
export async function installSkills({
  projectDir,
  state,
  aiFilesConfig,
}: {
  projectDir: string;
  state: AiFilesState;
  aiFilesConfig?: AiFilesProjectConfig | undefined;
}): Promise<boolean> {
  const agents = configuredSkillAgents(aiFilesConfig);
  if (agents.length === 0) return true;
  if (!(await shouldRunSkillsCli())) return true;

  logMessage("Installing Convex agent skills...");
  const skillsOk = await runSkillsAdd(projectDir, agents);
  if (!skillsOk) {
    logMessage(
      chalkStderr.yellow(
        "Could not install agent skills. You can retry manually with: npx skills add convex/agent-skills --copy",
      ),
    );
    return false;
  }

  const sha = await fetchAgentSkillsSha();
  if (sha) state.agentSkillsSha = sha;

  logMessage(`${chalkStderr.green("✔")} Skills installed`);
  return true;
}

export type RemoveInstalledSkillsStatus = "unchanged" | "removed" | "failed";

/**
 * Remove Convex-managed agent skills and clean up the lock file if empty.
 * Returns whether removal was skipped, succeeded, or failed.
 */
export async function removeInstalledSkills({
  projectDir,
  skillNames,
}: {
  projectDir: string;
  skillNames: string[];
}): Promise<RemoveInstalledSkillsStatus> {
  if (skillNames.length === 0) return "unchanged";
  if (!(await shouldRunSkillsCli())) return "unchanged";

  logMessage(`Removing Convex agent skills: ${skillNames.join(", ")}`);
  const skillsOk = await runSkillsRemove({ cwd: projectDir, skillNames });
  if (!skillsOk) {
    logMessage(
      chalkStderr.yellow(
        "Could not remove agent skills automatically. Remove them manually with: npx skills remove",
      ),
    );
    return "failed";
  }

  const lockRemoved = await removeSkillsLockIfEmpty({
    projectDir,
    removedSkillNames: skillNames,
  });

  if (lockRemoved)
    logMessage(`${chalkStderr.green("✔")} Deleted skills-lock.json.`);

  return "removed";
}

function runSkillsCommand(
  cwd: string,
  args: string[],
): Promise<{ ok: boolean; output: string }> {
  return new Promise((resolve) => {
    const proc = child_process.spawn(
      "npx",
      ["--yes", "skills@latest", ...args],
      {
        cwd,
        stdio: "pipe",
        // .cmd files on Windows require shell execution.
        shell: process.platform === "win32",
      },
    );
    let capturedOutput = "";
    proc.stdout?.on("data", (chunk) => {
      capturedOutput += chunk.toString();
    });
    proc.stderr?.on("data", (chunk) => {
      capturedOutput += chunk.toString();
    });
    proc.on("close", (code) => {
      if (code !== 0 && capturedOutput.trim().length > 0) {
        const lines = capturedOutput.trim().split(/\r?\n/);
        const tail = lines.slice(-10).join("\n");
        logMessage(chalkStderr.gray(`skills output (tail):\n${tail}`));
      }
      resolve({ ok: code === 0, output: capturedOutput });
    });
    proc.on("error", () => resolve({ ok: false, output: capturedOutput }));
  });
}
