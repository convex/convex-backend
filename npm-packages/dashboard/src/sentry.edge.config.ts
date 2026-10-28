// Sentry initialization for the Edge runtime, loaded from
// `instrumentation.ts`'s `register()`.
// https://docs.sentry.io/platforms/javascript/guides/nextjs/

import * as Sentry from "@sentry/nextjs";

const SENTRY_DSN = process.env.SENTRY_DSN || process.env.NEXT_PUBLIC_SENTRY_DSN;
const environment =
  process.env.NEXT_PUBLIC_ENVIRONMENT === "production"
    ? "production"
    : "development";

Sentry.init({
  dsn: SENTRY_DSN,
  tracesSampleRate: 0.01,
  environment,
  integrations: [
    Sentry.requestDataIntegration({
      include: {
        cookies: false,
      },
    }),
  ],
});
