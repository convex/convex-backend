import { defineApp } from "convex/server";
import { v } from "convex/values";

const app = defineApp({
  env: {
    // Clerk Issuer URL from your "convex" JWT template.
    CLERK_JWT_ISSUER_DOMAIN: v.string(),
    // Signing secret of the Clerk webhook endpoint, starting with `whsec_`.
    CLERK_WEBHOOK_SECRET: v.string(),
  },
});

export default app;
