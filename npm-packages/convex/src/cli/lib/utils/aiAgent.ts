import { determineAgent } from "@vercel/detect-agent";

/**
 * Returns the name of the AI agent running this process, or null if none is
 * detected. `@vercel/detect-agent` recognizes the environment variables that
 * agents like Claude Code, Codex and Cursor set; `CONVEX_AGENT_MODE` is
 * Convex's own.
 */
export async function detectAiAgent(): Promise<string | null> {
  if (process.env.CONVEX_AGENT_MODE) {
    return "CONVEX_AGENT_MODE";
  }
  const result = await determineAgent();
  return result.isAgent ? result.agent.name : null;
}
