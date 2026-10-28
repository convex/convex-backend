import { AuthConfig } from "convex/server";
import { env } from "./_generated/server";

export default {
  providers: [
    {
      domain: env.CONVEX_SITE_URL,
      applicationID: "convex",
    },
  ],
} satisfies AuthConfig;
