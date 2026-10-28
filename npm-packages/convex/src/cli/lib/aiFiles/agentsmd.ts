import {
  AGENTS_MD_START_MARKER,
  AGENTS_MD_END_MARKER,
  agentsMdConvexSection,
} from "../../codegen_templates/agentsmd.js";
import { agentsMdPath } from "./paths.js";
import { type AiFilesState } from "./state.js";
import {
  type ManagedSectionTarget,
  type InjectResult,
  type StripResult,
  injectManagedSection,
  attemptToStripManagedSection,
  hasManagedSection,
  attemptToRemoveMarkdownSection,
} from "./utils.js";
import { logMessage } from "../../../bundler/log.js";
import { chalkStderr } from "chalk";

function target(projectDir?: string): ManagedSectionTarget {
  return {
    filePath: agentsMdPath(projectDir),
    startMarker: AGENTS_MD_START_MARKER,
    endMarker: AGENTS_MD_END_MARKER,
  };
}

export async function injectAgentsMdSection({
  section,
  projectDir,
}: {
  section: string;
  projectDir?: string;
}): Promise<InjectResult> {
  return injectManagedSection({ ...target(projectDir), section });
}

export async function attemptToStripAgentsMdSection(
  projectDir: string,
): Promise<StripResult> {
  return attemptToStripManagedSection(target(projectDir));
}

export async function attemptToRemoveAgentsMdSection(
  projectDir: string,
): Promise<boolean> {
  return attemptToRemoveMarkdownSection({
    projectDir,
    strip: attemptToStripAgentsMdSection,
    fileName: "AGENTS.md",
  });
}

export async function hasAgentsMdInstalled(
  projectDir: string,
): Promise<boolean> {
  return hasManagedSection(target(projectDir));
}

/**
 * Inject (or update) the Convex section in AGENTS.md and record the hash.
 * Returns true if the file was actually written.
 */
export async function applyAgentsMdSection({
  projectDir,
  state,
  convexDirName,
}: {
  projectDir: string;
  state: AiFilesState;
  convexDirName: string;
}): Promise<boolean> {
  const result = await injectAgentsMdSection({
    section: agentsMdConvexSection(convexDirName),
    projectDir,
  });
  if (result.didWrite)
    logMessage(`${chalkStderr.green("✔")} AGENTS.md written`);
  state.agentsMdSectionHash = result.sectionHash;
  return result.didWrite;
}
