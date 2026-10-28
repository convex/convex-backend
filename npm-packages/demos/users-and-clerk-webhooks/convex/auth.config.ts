import { AuthConfig } from "convex/server";
import { env } from "./_generated/server";

export default {
  providers: [
    {
      domain: env.CLERK_JWT_ISSUER_DOMAIN,
      applicationID: "convex",
    },
  ],
} satisfies AuthConfig;
