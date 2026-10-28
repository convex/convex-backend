import { defineApp } from "convex/server";
import { v } from "convex/values";
import resend from "@convex-dev/resend/convex.config.js";

const app = defineApp({
  env: {
    RESEND_API_KEY: v.string(),
    RESEND_WEBHOOK_SECRET: v.string(),

    LOG_LEVEL: v.optional(v.union(v.literal("info"), v.literal("error"))),
    // @skipNextLine
    CLERK_JWT_ISSUER_DOMAIN: v.string(),
  },
});

app.use(resend, {
  // Serve the Resend component’s HTTP routes
  // at `https://<deployment>.convex.site/resend/`
  httpPrefix: "/resend/",
  env: {
    RESEND_API_KEY: app.env.RESEND_API_KEY,
    RESEND_WEBHOOK_SECRET: app.env.RESEND_WEBHOOK_SECRET,
  },
});

export default app;
