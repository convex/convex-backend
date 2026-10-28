import { Infer } from "convex/values";
import { ReactNode } from "react";
import * as Yup from "yup";
import {
  DatadogSiteLocation,
  ExportIntegrationType,
  ImportIntegrationType,
  Integration,
  IntegrationConfig,
  IntegrationType,
  RedactedS3ExportConfig,
} from "system-udfs/convex/_system/frontend/common";
import {
  axiomConfig,
  datadogConfig,
  postHogErrorTrackingConfig,
  postHogLogsConfig,
  sentryConfig,
  webhookConfig,
} from "system-udfs/convex/schema";
import { Link } from "@ui/Link";
import classNames from "classnames";
import { ArchiveBoxIcon } from "@heroicons/react/24/outline";
import { WebhookIcon } from "@common/elements/icons";
import { DatadogLogo } from "@common/lib/logos/DatadogLogo";
import { AxiomLogo } from "@common/lib/logos/AxiomLogo";
import { SentryLogo } from "@common/lib/logos/SentryLogo";
import { PostHogLogo } from "@common/lib/logos/PostHogLogo";
import { AirbyteLogo } from "@common/lib/logos/AirbyteLogo";
import { FivetranLogo } from "@common/lib/logos/FivetranLogo";
import { WorkosLogo } from "./logos/WorkosLogo";

// A configured integration narrowed to a single kind, as `listConfiguredSinks`
// returns it. Derives fields from the schema.
type LogSinkDoc<T extends IntegrationConfig["type"]> = Omit<
  Integration,
  "config"
> & { config: Extract<IntegrationConfig, { type: T }> };

export const topicsValidationSchema = Yup.array()
  .nullable()
  .test(
    "non-empty-topics",
    "Select at least one topic.",
    (value) => value === null || value === undefined || value.length > 0,
  );

export const LOG_INTEGRATIONS = [
  "axiom",
  "datadog",
  "webhook",
  "postHogLogs",
] as const;
export const EXC_INTEGRATIONS = ["sentry", "postHogErrorTracking"] as const;
export const ANALYTICS_INTEGRATIONS = ["s3Export"] as const;
export const EXPORT_INTEGRATIONS: ExportIntegrationType[] = ["fivetran"];
export const IMPORT_INTEGRATIONS: ImportIntegrationType[] = ["airbyte"];

export type LogIntegrationConfig =
  | Infer<typeof axiomConfig>
  | Infer<typeof datadogConfig>
  | Infer<typeof webhookConfig>
  | Infer<typeof postHogLogsConfig>;

export type LogIntegration =
  | { kind: "datadog"; existing: LogSinkDoc<"datadog"> | null }
  | { kind: "axiom"; existing: LogSinkDoc<"axiom"> | null }
  | { kind: "webhook"; existing: LogSinkDoc<"webhook"> | null }
  | { kind: "postHogLogs"; existing: LogSinkDoc<"postHogLogs"> | null };

export type ExceptionReportingIntegration =
  | { kind: "sentry"; existing: LogSinkDoc<"sentry"> | null }
  | {
      kind: "postHogErrorTracking";
      existing: LogSinkDoc<"postHogErrorTracking"> | null;
    };

export type AnalyticsIntegration = {
  kind: "s3Export";
  existing: LogSinkDoc<"s3Export"> | null;
};

export type AnalyticsIntegrationConfig = RedactedS3ExportConfig;

export type ExceptionReportingIntegrationConfig =
  | Infer<typeof sentryConfig>
  | Infer<typeof postHogErrorTrackingConfig>;

export type AuthIntegration = {
  kind: "workos";
  existing: {
    workosEnvironmentId: string;
    workosEnvironmentName: string;
    workosClientId: string;
  } | null;
};

export function integrationToLogo(
  kind: IntegrationType,
  small?: boolean,
): {
  logo: ReactNode;
} {
  const sizeClass = small ? "size-5" : "size-10";
  const size = small ? 20 : 40;

  switch (kind) {
    case "datadog":
      return {
        logo: (
          <DatadogLogo
            className={classNames("rounded-sm border", sizeClass)}
            size={size}
          />
        ),
      };
    case "webhook":
      return {
        logo: (
          <div
            className={classNames(
              "flex items-center justify-center rounded-sm border",
              sizeClass,
            )}
          >
            <WebhookIcon className={small ? "size-4" : "size-7"} size={size} />
          </div>
        ),
      };
    case "axiom":
      return {
        logo: (
          <AxiomLogo
            className={classNames("rounded-sm border", sizeClass)}
            size={size}
          />
        ),
      };
    case "sentry":
      return {
        logo: (
          <SentryLogo
            className={classNames("rounded-sm border", sizeClass)}
            size={size}
          />
        ),
      };
    case "postHogLogs":
    case "postHogErrorTracking":
      return {
        logo: (
          <PostHogLogo
            className={classNames("rounded-sm border", sizeClass)}
            size={size}
          />
        ),
      };
    case "s3Export":
      return {
        logo: (
          <div
            className={classNames(
              "flex items-center justify-center rounded-sm border",
              sizeClass,
            )}
          >
            <ArchiveBoxIcon className={small ? "size-4" : "size-7"} />
          </div>
        ),
      };
    case "airbyte":
      return {
        logo: (
          <AirbyteLogo
            className={classNames("rounded-sm border", sizeClass)}
            size={size}
          />
        ),
      };
    case "fivetran":
      return {
        logo: (
          <FivetranLogo
            className={classNames(
              "rounded-sm border bg-white dark:bg-black",
              sizeClass,
            )}
            size={size}
          />
        ),
      };
    case "workos": {
      return {
        logo: (
          <WorkosLogo
            className={classNames(
              "rounded-sm border bg-white dark:bg-black",
              sizeClass,
            )}
            size={size}
          />
        ),
      };
    }
    default: {
      kind satisfies never;
      throw new Error(`Unrecognized integration type ${kind}`);
    }
  }
}

export function integrationUsingLegacyFormat(
  config:
    | LogIntegrationConfig
    | ExceptionReportingIntegrationConfig
    | AnalyticsIntegrationConfig
    | null,
) {
  if (config === null) {
    return false;
  }
  switch (config.type) {
    case "axiom":
      return config.version !== "2";
    case "datadog":
      return config.version !== "2";
    case "webhook":
      return false;
    case "sentry":
      return config.version !== "2";
    case "postHogLogs":
    case "postHogErrorTracking":
    case "s3Export":
      return false;
    default: {
      config satisfies never;
      return false;
    }
  }
}

export const LOG_STREAMS_DESCRIPTION = (
  <div>
    <p>Configure and monitor logging integrations within this deployment. </p>
    <p>
      Logs will be routed to your configured drains as functions are called and
      events occur in your deployment.
    </p>
    <p>
      This gives you full flexibility to query, store, and process logs as
      needed.
    </p>
  </div>
);

export const STREAMING_EXPORT_DESCRIPTION = (
  <div>
    <p>Set up streaming export with a third party connector platform.</p>{" "}
    <p>
      Fivetran is a data integration platform that allows you to export your
      Convex data to other databases and data warehouses like Snowflake,
      Databricks, BigTable, Tableau, and many others.
    </p>
    <Link
      passHref
      href="https://docs.convex.dev/database/import-export/streaming"
      target="_blank"
    >
      Learn more
    </Link>
    .
  </div>
);

export const ANALYTICS_EXPORT_DESCRIPTION = (
  <div>
    <p>
      Export this deployment's change log to an S3 bucket you own as Apache
      Iceberg tables, queryable from Athena, Snowflake, Databricks, and other
      engines through AWS Glue.
    </p>
  </div>
);

export const STREAMING_IMPORT_DESCRIPTION = (
  <div>
    <p>Set up streaming import with a third party connector platform.</p>{" "}
    <p>
      Airbyte is a data integration platform that allows you to import data from
      other databases and sources into Convex.
    </p>
    <Link
      passHref
      href="https://docs.convex.dev/database/import-export/streaming"
      target="_blank"
    >
      Learn more
    </Link>
    .
  </div>
);

export const AUTHENTICATION_DESCRIPTION = (
  <div>
    <p>
      An automatically provisioned WorkOS AuthKit environments for this
      deployment.
    </p>
  </div>
);

export type IntegrationUnavailableReason =
  | "MissingEntitlement"
  | "CannotManageDeployment"
  | "LocalDeployment";

export function configToUrl(config: IntegrationConfig): string {
  const kind = config.type;
  switch (kind) {
    case "sentry":
      return `https://sentry.io`;
    case "datadog":
      return datadogSiteLocationToUrl(config.siteLocation);
    case "axiom":
      return `https://app.axiom.co`;
    case "webhook":
      return config.url;
    case "postHogLogs": {
      const logsHost = (config.host ?? "https://us.i.posthog.com").replace(
        ".i.",
        ".",
      );
      return `${logsHost}/logs`;
    }
    case "postHogErrorTracking": {
      const etHost = (config.host ?? "https://us.i.posthog.com").replace(
        ".i.",
        ".",
      );
      return `${etHost}/error_tracking`;
    }
    case "s3Export":
      return `https://s3.console.aws.amazon.com/s3/buckets/${config.bucket}?region=${encodeURIComponent(config.region)}`;
    default:
      kind satisfies never;
      throw new Error(`Unrecognized integration type ${kind}`);
  }
}

function datadogSiteLocationToUrl(siteLocation: DatadogSiteLocation): string {
  switch (siteLocation) {
    case "US1":
      return "https://datadoghq.com";
    case "US3":
      return "https://us3.datadoghq.com";
    case "US5":
      return "https://us5.datadoghq.com";
    case "EU":
      return "https://datadoghq.eu";
    case "US1_FED":
      return "https://ddog-gov.com";
    case "AP1":
      return "https://ap1.datadoghq.com";
    default: {
      siteLocation satisfies never;
      throw new Error(`Unrecognized site location ${siteLocation}`);
    }
  }
}

export const integrationName = (kind: IntegrationType) => {
  switch (kind) {
    case "workos":
      return "WorkOS";
    case "postHogLogs":
      return "PostHog Logs";
    case "postHogErrorTracking":
      return "PostHog Error Tracking";
    case "s3Export":
      return "AWS S3";
    default:
      return kind.charAt(0).toUpperCase() + kind.slice(1);
  }
};
