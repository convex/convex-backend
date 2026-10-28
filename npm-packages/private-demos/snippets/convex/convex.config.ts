import { defineApp } from "convex/server";
import { v } from "convex/values";

const app = defineApp({
  env: {
    // Shared secret that callers of `loadOne` must pass as the `token` argument.
    CONVEX_AUTH_TOKEN: v.string(),
    // Optional: notifications are only sent when this is set
    SLACK_WEBHOOK_URL: v.optional(v.string()),
  },
});

export default app;
