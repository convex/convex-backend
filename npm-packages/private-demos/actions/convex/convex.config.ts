import { defineApp } from "convex/server";
import { v } from "convex/values";

// Only the langchain and email demos need these, so they are optional: the
// other actions in this playground work without them.
const app = defineApp({
  env: {
    OPENAI_API_KEY: v.optional(v.string()),
    POSTMARK_SERVER_TOKEN: v.optional(v.string()),
  },
});

export default app;
