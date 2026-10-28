import { AuthConfig } from "convex/server";
import { env } from "./_generated/server";

export default {
  providers: [
    // Setting up Convex Auth
    {
      domain: env.CONVEX_SITE_URL,
      applicationID: "convex",
    },

    // Setting up an OIDC provider (e.g. Auth0, Clerk, or custom)
    {
      domain: env.CLERK_JWT_ISSUER_DOMAIN,
      applicationID: "convex",
    },

    // Setting up a custom JWT provider
    {
      type: "customJwt",
      applicationID: "your-application-id",
      issuer: "https://your.issuer.url.com",
      jwks: "https://your.issuer.url.com/.well-known/jwks.json",
      algorithm: "RS256",
    },
  ],
} satisfies AuthConfig;
