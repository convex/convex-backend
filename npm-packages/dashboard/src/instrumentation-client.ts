// This file configures the initialization of Sentry on the browser.
// Next.js loads it before the app boots, on every page visit.
// https://docs.sentry.io/platforms/javascript/guides/nextjs/

import * as Sentry from "@sentry/nextjs";

const SENTRY_DSN = process.env.SENTRY_DSN || process.env.NEXT_PUBLIC_SENTRY_DSN;
const environment =
  process.env.NEXT_PUBLIC_ENVIRONMENT === "production"
    ? "production"
    : "development";

Sentry.init({
  dsn: SENTRY_DSN,
  profilesSampleRate: 0.05,
  tracesSampleRate: 0.05,
  tunnel:
    environment === "production"
      ? `${process.env.NEXT_PUBLIC_BIG_BRAIN_URL}/sentry`
      : undefined,
  environment,
  integrations: [Sentry.browserTracingIntegration()],
  // Which outgoing requests get `sentry-trace`/`baggage` headers. Matched
  // against the fully resolved URL, plus the pathname for same-origin
  // requests — hence `/^\//` for our own API routes.
  tracePropagationTargets: ["localhost", /^\//, /.*\.convex.cloud/],
  release: process.env.NEXT_PUBLIC_VERCEL_GIT_COMMIT_SHA,
  ignoreErrors: [
    "ResizeObserver loop completed with undelivered notifications.",
    "ConvexReactClient has already been closed.",
    /.*AccessTokenInvalid.*/,
  ],
});

// Only the App Router calls this; exported so the SDK does not warn, and so
// navigations are instrumented if an `app/` directory is ever added.
export const onRouterTransitionStart = Sentry.captureRouterTransitionStart;
