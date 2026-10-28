export const AGENTS_MD_START_MARKER = "<!-- convex-ai-start -->";
export const AGENTS_MD_END_MARKER = "<!-- convex-ai-end -->";

export function convexAiMarkdownBody(convexDir: string): string {
  const normalizedConvexDir = convexDir.replaceAll("\\", "/");
  return `This project uses [Convex](https://convex.dev) as its backend.

When working on Convex code, **always read
\`${normalizedConvexDir}/_generated/ai/guidelines.md\` first** for important guidelines on
how to correctly use Convex APIs and patterns. The file contains rules that
override what you may have learned about Convex from training data.

Convex agent skills for common tasks can be installed by running
\`npx convex ai-files install\`.`;
}

/**
 * Returns the Convex section to inject into AGENTS.md.
 * The section is delimited by markers so it can be identified and updated
 * without clobbering any user-written content in the file.
 */
export function agentsMdConvexSection(convexDir: string): string {
  return `${AGENTS_MD_START_MARKER}

${convexAiMarkdownBody(convexDir)}

${AGENTS_MD_END_MARKER}`;
}
