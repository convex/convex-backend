import { convexAiMarkdownBody } from "./agentsmd.js";

/**
 * Markers delimiting the Convex-managed section in CLAUDE.md.
 * Everything outside this block is user-owned and left untouched.
 */
export const CLAUDE_MD_START_MARKER = "<!-- convex-ai-start -->";
export const CLAUDE_MD_END_MARKER = "<!-- convex-ai-end -->";

/**
 * Returns the Convex section to inject into CLAUDE.md.
 */
export function claudeMdConvexSection(convexDir: string): string {
  return `${CLAUDE_MD_START_MARKER}

${convexAiMarkdownBody(convexDir)}

${CLAUDE_MD_END_MARKER}`;
}
