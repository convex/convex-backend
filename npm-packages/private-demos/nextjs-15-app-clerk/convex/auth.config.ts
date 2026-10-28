import { AuthConfig } from "convex/server";
import { env } from "./_generated/server";

export default {
  providers: [
    {
      // Set CLERK_JWT_ISSUER_DOMAIN on the Convex dashboard to the Issuer URL
      // of your "convex" JWT template.
      // See https://docs.convex.dev/auth/clerk#configuring-dev-and-prod-instances
      domain: env.CLERK_JWT_ISSUER_DOMAIN,
      applicationID: "convex",
    },
  ],
} satisfies AuthConfig;
