import path from "path";
import { chalkStderr } from "chalk";
import { logMessage } from "../../../bundler/log.js";
import { safelyDeleteFile } from "./utils.js";

/**
 * Remove the legacy `.cursor/rules/convex_rules.mdc` file if it exists.
 * This file was written by the old cursor rules auto-update feature (removed
 * in favour of the AI files system). We clean it up unconditionally
 * during `writeAiFiles`, `convex ai-files update`, and `convex ai-files remove`
 * since it was always auto-managed and is now superseded by
 * `convex/_generated/ai/guidelines.md`.
 */
export async function removeLegacyCursorRulesFile(
  projectDir: string,
): Promise<boolean> {
  const removed = await safelyDeleteFile(
    path.join(projectDir, ".cursor", "rules", "convex_rules.mdc"),
  );
  if (removed)
    logMessage(
      `${chalkStderr.green("✔")} Removed legacy .cursor/rules/convex_rules.mdc (superseded by convex/_generated/ai/guidelines.md).`,
    );
  return removed;
}
