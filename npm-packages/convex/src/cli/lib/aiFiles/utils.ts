// eslint-disable-next-line no-restricted-imports
import { promises as fs } from "fs";
import { chalkStderr } from "chalk";
import { logMessage } from "../../../bundler/log.js";
import { hashSha256 } from "../utils/hash.js";

export function isInInteractiveTerminal(): boolean {
  return process.stdin.isTTY === true;
}

export type ReadFileResult =
  | { kind: "not-found" }
  | { kind: "empty" }
  | { kind: "content"; content: string };

export async function attemptReadFile(
  filePath: string,
): Promise<ReadFileResult> {
  try {
    const content = await fs.readFile(filePath, "utf8");
    if (content.length === 0) return { kind: "empty" };
    return { kind: "content", content };
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === "ENOENT")
      return { kind: "not-found" };

    // eslint-disable-next-line no-restricted-syntax
    throw error;
  }
}

export async function readFileOrNull(filePath: string): Promise<string | null> {
  const result = await attemptReadFile(filePath);
  if (result.kind === "content") return result.content;
  if (result.kind === "empty") return "";
  if (result.kind === "not-found") return null;
  return exhaustiveCheck(result);
}

/**
 * Attempt to delete a file. Returns `true` if the file was deleted,
 * `false` if it didn't exist or the deletion failed.
 */
export async function safelyDeleteFile(filePath: string): Promise<boolean> {
  try {
    await fs.unlink(filePath);
    return true;
  } catch {
    return false;
  }
}

// ---------------------------------------------------------------------------
// Generic managed-section helpers
//
// Several files (AGENTS.md, CLAUDE.md) contain a Convex-managed section
// delimited by start/end markers. These helpers provide the common logic
// for injecting, stripping, and detecting those sections.
// ---------------------------------------------------------------------------

export type ManagedSectionTarget = {
  filePath: string;
  startMarker: string;
  endMarker: string;
};

export type InjectResult = {
  sectionHash: string;
  didWrite: boolean;
};

export const iife = <T>(fn: () => T): T => fn();

/**
 * Inject a managed section into a file. If the file already contains the
 * markers, the section between them is replaced. Otherwise the section is
 * appended (or the file is created). Only writes when content actually
 * changes.
 */
export async function injectManagedSection(
  opts: ManagedSectionTarget & { section: string },
): Promise<InjectResult> {
  const { filePath, startMarker, endMarker, section } = opts;

  const existing = (await readFileOrNull(filePath)) ?? "";

  const startIdx = existing.indexOf(startMarker);
  const endIdx = existing.indexOf(endMarker);

  const updated = iife(() => {
    if (startIdx !== -1 && endIdx !== -1)
      return (
        existing.slice(0, startIdx) +
        section +
        existing.slice(endIdx + endMarker.length)
      );
    if (existing.length > 0)
      return existing.trimEnd() + "\n\n" + section + "\n";

    return section + "\n";
  });

  const didWrite = updated !== existing;
  if (didWrite) await fs.writeFile(filePath, updated, "utf8");

  return { sectionHash: hashSha256(section), didWrite };
}

export type StripResult = "none" | "section" | "file";

/**
 * Remove the managed section (between start/end markers) from a file.
 * If the file is empty after removal, it is deleted.
 *
 * Returns `"none"` if the file doesn't exist or has no markers,
 * `"section"` if the section was stripped, or `"file"` if the entire
 * file was deleted.
 */
export async function attemptToStripManagedSection(
  opts: ManagedSectionTarget,
): Promise<StripResult> {
  const { filePath, startMarker, endMarker } = opts;

  const content = await readFileOrNull(filePath);
  if (content === null) return "none";

  const startIdx = content.indexOf(startMarker);
  const endIdx = content.indexOf(endMarker);
  if (startIdx === -1 || endIdx === -1) {
    return "none";
  }

  const before = content.slice(0, startIdx).trimEnd();
  const after = content.slice(endIdx + endMarker.length).trimStart();
  const updated = [before, after].filter(Boolean).join("\n\n");

  if (!updated.trim()) {
    await safelyDeleteFile(filePath);
    return "file";
  }

  await fs.writeFile(filePath, updated + "\n", "utf8");
  return "section";
}

export function exhaustiveCheck(_param: never): never {
  // eslint-disable-next-line no-restricted-syntax
  throw new Error("Internal error: exhaustive check failed.");
}

export async function attemptToRemoveMarkdownSection({
  projectDir,
  strip,
  fileName,
}: {
  projectDir: string;
  strip: (dir: string) => Promise<StripResult>;
  fileName: string;
}): Promise<boolean> {
  const result = await strip(projectDir);

  if (result === "section") {
    logMessage(
      `${chalkStderr.green("✔")} Removed Convex section from ${fileName}.`,
    );
    return true;
  }

  if (result === "file") {
    logMessage(`${chalkStderr.green("✔")} Deleted ${fileName}.`);
    return true;
  }

  if (result === "none") return false;

  return exhaustiveCheck(result);
}

/**
 * Check whether a file contains a managed section (both markers present).
 */
export async function hasManagedSection(
  opts: ManagedSectionTarget,
): Promise<boolean> {
  const content = await readFileOrNull(opts.filePath);
  return (
    content !== null &&
    content.includes(opts.startMarker) &&
    content.includes(opts.endMarker)
  );
}
