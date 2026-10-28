import { defineApp } from "convex/server";
import { v } from "convex/values";

// Optional so the rest of the showcase (node_fetch, sharp, tiktoken) can be
// pushed without Snowflake credentials.
const app = defineApp({
  env: {
    SNOWFLAKE_ACCOUNT: v.optional(v.string()),
    SNOWFLAKE_USERNAME: v.optional(v.string()),
    SNOWFLAKE_PASSWORD: v.optional(v.string()),
  },
});

export default app;
