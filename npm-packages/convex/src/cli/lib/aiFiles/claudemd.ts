import {
  CLAUDE_MD_END_MARKER,
  CLAUDE_MD_START_MARKER,
  claudeMdConvexSection,
} from "../../codegen_templates/claudemd.js";
import { claudeMdPath } from "./paths.js";
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
    filePath: claudeMdPath(projectDir),
    startMarker: CLAUDE_MD_START_MARKER,
    endMarker: CLAUDE_MD_END_MARKER,
  };
}

export async function injectClaudeMdSection({
  section,
  projectDir,
}: {
  section: string;
  projectDir?: string;
}): Promise<InjectResult> {
  return injectManagedSection({ ...target(projectDir), section });
}

export async function attemptToStripClaudeMdSection(
  projectDir: string,
): Promise<StripResult> {
  return attemptToStripManagedSection(target(projectDir));
}

export async function attemptToRemoveClaudeMdSection(
  projectDir: string,
): Promise<boolean> {
  return attemptToRemoveMarkdownSection({
    projectDir,
    strip: attemptToStripClaudeMdSection,
    fileName: "CLAUDE.md",
  });
}

export async function hasClaudeMdInstalled(
  projectDir: string,
): Promise<boolean> {
  return hasManagedSection(target(projectDir));
}

/**
 * Inject (or update) the Convex section in CLAUDE.md and record the hash.
 * Returns true if the file was actually written.
 */
export async function applyClaudeMdSection({
  projectDir,
  state,
  convexDirName,
}: {
  projectDir: string;
  state: AiFilesState;
  convexDirName: string;
}): Promise<boolean> {
  const result = await injectClaudeMdSection({
    section: claudeMdConvexSection(convexDirName),
    projectDir,
  });
  if (result.didWrite)
    logMessage(`${chalkStderr.green("✔")} CLAUDE.md written`);
  state.claudeMdHash = result.sectionHash;
  return result.didWrite;
}
