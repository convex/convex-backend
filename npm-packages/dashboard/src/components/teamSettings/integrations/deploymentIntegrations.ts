import { IntegrationType } from "system-udfs/convex/_system/frontend/common";

const LOG_STREAMS_DOCS =
  "https://docs.convex.dev/production/integrations/log-streams/";
const EXCEPTION_REPORTING_DOCS =
  "https://docs.convex.dev/production/integrations/exception-reporting";
const STREAMING_DOCS =
  "https://docs.convex.dev/database/import-export/streaming";

// Deployment integrations as the team catalog lists them, in catalog order.
// Each is configured on a deployment's Settings › Integrations page. WorkOS
// connects a workspace to the whole team, so the catalog shows it team-wide.
export const DEPLOYMENT_INTEGRATIONS: Record<
  Exclude<IntegrationType, "workos">,
  { description: string; docsUrl: string }
> = {
  datadog: {
    description: "Stream logs from your functions to Datadog.",
    docsUrl: LOG_STREAMS_DOCS,
  },
  axiom: {
    description: "Stream logs from your functions to Axiom.",
    docsUrl: LOG_STREAMS_DOCS,
  },
  postHogLogs: {
    description: "Stream logs from your functions to PostHog.",
    docsUrl: LOG_STREAMS_DOCS,
  },
  webhook: {
    description: "Send logs from your functions to any HTTP endpoint.",
    docsUrl: `${LOG_STREAMS_DOCS}#webhook`,
  },
  sentry: {
    description: "Report exceptions thrown by your functions to Sentry.",
    docsUrl: EXCEPTION_REPORTING_DOCS,
  },
  postHogErrorTracking: {
    description:
      "Report exceptions thrown by your functions to PostHog Error Tracking.",
    docsUrl: EXCEPTION_REPORTING_DOCS,
  },
  fivetran: {
    description:
      "Export your data to warehouses like Snowflake and Databricks with Fivetran.",
    docsUrl: STREAMING_DOCS,
  },
  airbyte: {
    description: "Import data from other databases into Convex with Airbyte.",
    docsUrl: STREAMING_DOCS,
  },
  s3Export: {
    description:
      "Export your data to an S3 bucket you own as Apache Iceberg tables.",
    docsUrl: "https://docs.convex.dev/production/integrations/s3-export",
  },
};
