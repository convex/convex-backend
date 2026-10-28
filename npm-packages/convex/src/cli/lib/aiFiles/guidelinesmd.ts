// eslint-disable-next-line no-restricted-imports
import { promises as fs } from "fs";
import { chalkStderr } from "chalk";
import { logMessage } from "../../../bundler/log.js";
import { downloadGuidelines } from "../versionApi.js";
import { hashSha256 } from "../utils/hash.js";
import { aiDirForConvexDir, guidelinesPathForConvexDir } from "./paths.js";
import { attemptReadFile, exhaustiveCheck } from "./utils.js";
import { type AiFilesState } from "./state.js";

export async function hasGuidelinesInstalled(
  convexDir: string,
): Promise<boolean> {
  const result = await attemptReadFile(guidelinesPathForConvexDir(convexDir));
  if (result.kind === "content") return true;
  if (result.kind === "empty" || result.kind === "not-found") return false;
  return exhaustiveCheck(result);
}

/**
 * Download and write the guidelines file.
 * Guidelines live in `_generated/` so local edits are not expected and are
 * not preserved.
 *
 * Returns false if the guidelines could not be downloaded.
 */
export async function installGuidelinesFile({
  convexDir,
  state,
}: {
  convexDir: string;
  state: AiFilesState;
}): Promise<boolean> {
  const guidelines = await downloadGuidelines();

  if (guidelines === null) {
    logMessage(
      chalkStderr.yellow(
        "Could not download Convex AI guidelines right now. You can retry with: npx convex ai-files install",
      ),
    );
    return false;
  }

  await fs.mkdir(aiDirForConvexDir(convexDir), { recursive: true });
  await fs.writeFile(guidelinesPathForConvexDir(convexDir), guidelines, "utf8");
  state.guidelinesHash = hashSha256(guidelines);

  logMessage(
    `${chalkStderr.green("✔")} ${guidelinesPathForConvexDir(convexDir)} written`,
  );

  return true;
}
