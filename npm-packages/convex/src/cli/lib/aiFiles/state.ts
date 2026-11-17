import * as Sentry from "@sentry/node";
// Use raw fs (not ctx.fs) so these operations run asynchronously and don't
// interfere with the file-watcher used by `convex dev`.
// eslint-disable-next-line no-restricted-imports
import { promises as fs } from "fs";
import { z } from "zod";
import { aiDirForConvexDir, aiFilesStatePathForConvexDir } from "./paths.js";
import { attemptReadFile, exhaustiveCheck } from "./utils.js";

export const aiFilesStateSchema = z.object({
  guidelinesHash: z.string().nullable(),
  agentsMdSectionHash: z.string().nullable(),
  claudeMdHash: z.string().nullable(),
  // Commit SHA from convex/agent-skills that was current when skills were
  // last installed. Used to detect when newer skills are available.
  agentSkillsSha: z.string().nullable(),
});

export type AiFilesState = z.infer<typeof aiFilesStateSchema>;

const DEFAULT_AI_STATE: AiFilesState = {
  guidelinesHash: null,
  agentsMdSectionHash: null,
  claudeMdHash: null,
  agentSkillsSha: null,
};

export type AttemptReadAiStateResult =
  | { kind: "no-file" }
  | { kind: "ok"; state: AiFilesState }
  | { kind: "parse-error"; error: unknown };

export async function attemptReadAiState(
  convexDir: string,
): Promise<AttemptReadAiStateResult> {
  const result = await attemptReadFile(aiFilesStatePathForConvexDir(convexDir));
  if (result.kind === "not-found" || result.kind === "empty")
    return { kind: "no-file" };

  try {
    const state = aiFilesStateSchema.parse(JSON.parse(result.content));
    return { kind: "ok", state };
  } catch (error) {
    Sentry.captureException(error);
    return { kind: "parse-error", error };
  }
}

export async function readAiStateOrDefault(
  convexDir: string,
): Promise<AiFilesState> {
  const result = await attemptReadAiState(convexDir);
  if (result.kind === "ok") return result.state;
  if (result.kind === "no-file") return { ...DEFAULT_AI_STATE };
  if (result.kind === "parse-error") return { ...DEFAULT_AI_STATE };
  return exhaustiveCheck(result);
}

export async function hasAiState(convexDir: string): Promise<boolean> {
  const result = await attemptReadAiState(convexDir);
  return result.kind === "ok";
}

export async function writeAiState({
  state,
  convexDir,
}: {
  state: AiFilesState;
  convexDir: string;
}): Promise<void> {
  const validated = aiFilesStateSchema.parse(state);
  await fs.mkdir(aiDirForConvexDir(convexDir), { recursive: true });
  await fs.writeFile(
    aiFilesStatePathForConvexDir(convexDir),
    JSON.stringify(validated, null, 2) + "\n",
    "utf8",
  );
}
