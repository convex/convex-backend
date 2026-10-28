import * as Sentry from "@sentry/node";

const SENTRY_DSN =
  "https://071567405e8a93c387ba63b40730a43d@o1192621.ingest.us.sentry.io/4512330271621120";

export function initSentry(version: string) {
  if (process.env.CI) {
    return;
  }
  Sentry.init({
    dsn: SENTRY_DSN,
    release: "codemod@" + version,
  });
}
