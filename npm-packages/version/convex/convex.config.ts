import { defineApp } from "convex/server";
import { v } from "convex/values";

const app = defineApp({
  env: {
    // Bearer token the agent skills publisher must present to /v1/agent_skills.
    // Unset means the sync endpoint rejects every request.
    AGENT_SKILLS_SYNC_TOKEN: v.optional(v.string()),
    // Only raises the GitHub API rate limit; requests work unauthenticated.
    GITHUB_TOKEN: v.optional(v.string()),
  },
});

export default app;
