import path from "path";

export type AiFilesPaths = {
  projectDir: string;
  convexDir: string;
};

const AI_FILES_PARENT_DIR = "_generated";
const AI_FILES_DIR = "ai";

export function aiDirForConvexDir(convexDir: string): string {
  return path.join(convexDir, AI_FILES_PARENT_DIR, AI_FILES_DIR);
}

export function guidelinesPathForConvexDir(convexDir: string): string {
  return path.join(aiDirForConvexDir(convexDir), "guidelines.md");
}

export function aiFilesStatePathForConvexDir(convexDir: string): string {
  return path.join(aiDirForConvexDir(convexDir), "ai-files.state.json");
}

export function agentsMdPath(projectDir?: string): string {
  return path.join(projectDir ?? process.cwd(), "AGENTS.md");
}

export function claudeMdPath(projectDir?: string): string {
  return path.join(projectDir ?? process.cwd(), "CLAUDE.md");
}
