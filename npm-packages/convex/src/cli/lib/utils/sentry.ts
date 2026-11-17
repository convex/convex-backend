import { productionProvisionHost, provisionHost } from "../config.js";
import * as Sentry from "@sentry/node";
import { version } from "../../../index.js";
import { stripVTControlCharacters } from "util";
import { ZodError } from "zod";

export const SENTRY_DSN =
  "https://f9fa0306e3d540079cf40ce8c2ad9644@o1192621.ingest.sentry.io/6390839";

export const LOCAL_BACKEND_SENTRY_DSN =
  "https://1c77542933b4002e48934a351c84dd38@o1192621.ingest.us.sentry.io/4511826465587200";

export function initSentry() {
  if (
    (!process.env.CI || process.env.VERCEL === "1") &&
    provisionHost === productionProvisionHost
  ) {
    Sentry.init({
      dsn: SENTRY_DSN,
      release: "cli@" + version,
      tracesSampleRate: 0.2,
      beforeSend: (event, hint) => {
        if (hint.originalException instanceof ZodError) {
          event.fingerprint = zodErrorFingerprint(hint.originalException);
        }
        return event;
      },
      beforeBreadcrumb: (breadcrumb) => {
        // Strip ANSI color codes from log lines that are sent as breadcrumbs.
        if (breadcrumb.message) {
          breadcrumb.message = stripVTControlCharacters(breadcrumb.message);
        }
        return breadcrumb;
      },
    });
  }
}

const IDENTIFIER = /^[A-Za-z_][A-Za-z0-9_]*$/;

/**
 * Sentry groups every `ZodError` under one issue because they share a stack
 * trace. Fingerprinting by each issue's code and path gives each distinct
 * response mismatch its own Sentry issue. Path segments that aren't
 * identifiers, such as array indices and component paths, are replaced by
 * `*` so that the same mismatch in different projects groups together.
 */
export function zodErrorFingerprint(error: ZodError): string[] {
  const issues = error.issues.map(
    (issue) =>
      `${issue.code}:${issue.path
        .map((segment) =>
          typeof segment === "string" && IDENTIFIER.test(segment)
            ? segment
            : "*",
        )
        .join(".")}`,
  );
  return ["ZodError", ...[...new Set(issues)].sort()];
}
