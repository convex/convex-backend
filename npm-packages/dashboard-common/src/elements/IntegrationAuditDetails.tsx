import { Infer } from "convex/values";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import { ChevronDownIcon, ChevronUpIcon } from "@radix-ui/react-icons";
import {
  integrationAuditConfig,
  integrationAuditCredentialChanges,
} from "system-udfs/convex/tableDefs/deploymentAuditLogTable";

type Config = Infer<typeof integrationAuditConfig>;
type CredentialChanges = Infer<typeof integrationAuditCredentialChanges>;
type Field = { label: string; value: string };

function topicsLabel(topics: string[] | null): string {
  return topics === null
    ? "All topics, including future topics"
    : topics.join(", ") || "None";
}

function configurationFields(config: Config): Field[] {
  switch (config.type) {
    case "local":
      return [{ label: "Path", value: config.path }];
    case "datadog":
      return [
        { label: "Site", value: config.site },
        { label: "Service", value: config.service ?? "(default)" },
        { label: "Tags", value: config.tags.join(", ") || "None" },
        { label: "Format version", value: config.format_version },
        { label: "Topics", value: topicsLabel(config.topics) },
      ];
    case "axiom":
      return [
        { label: "Dataset", value: config.dataset },
        {
          label: "Ingest origin",
          value: config.ingest_origin ?? "(unavailable)",
        },
        { label: "Format version", value: config.format_version },
        { label: "Topics", value: topicsLabel(config.topics) },
      ];
    case "webhook":
      return [
        { label: "URL origin", value: config.url_origin ?? "(unavailable)" },
        { label: "Format", value: config.format },
        { label: "Topics", value: topicsLabel(config.topics) },
      ];
    case "sentry":
      return [
        { label: "DSN origin", value: config.dsn_origin ?? "(unavailable)" },
        { label: "Project ID", value: config.project_id },
        { label: "Format version", value: config.format_version },
      ];
    case "postHogLogs":
      return [
        { label: "Host origin", value: config.host_origin ?? "(unavailable)" },
        { label: "Service", value: config.service_name ?? "(default)" },
        { label: "Topics", value: topicsLabel(config.topics) },
      ];
    case "postHogErrorTracking":
      return [
        { label: "Host origin", value: config.host_origin ?? "(unavailable)" },
      ];
    case "s3Export":
      return [
        { label: "Bucket", value: config.bucket },
        { label: "Region", value: config.region },
        { label: "Prefix", value: config.prefix ?? "(bucket root)" },
        { label: "Schedule", value: config.period },
      ];
    default:
      config satisfies never;
      return [];
  }
}

function configurationDetails(
  config?: Config,
): { label: string; value: unknown } | undefined {
  switch (config?.type) {
    case "s3Export":
      return { label: "Export selection", value: config.selection };
    case "axiom":
      return { label: "Attributes", value: config.attributes };
    case "sentry":
      return { label: "Tags", value: config.tags ?? {} };
    case "local":
    case "datadog":
    case "webhook":
    case "postHogLogs":
    case "postHogErrorTracking":
    case undefined:
      return undefined;
    default:
      config satisfies never;
      return undefined;
  }
}

function sensitiveChanges(changes?: CredentialChanges): string[] {
  if (!changes) return [];
  let fields: [string, boolean][];
  switch (changes.type) {
    case "datadog":
      fields = [["API key", changes.api_key_changed]];
      break;
    case "axiom":
      fields = [
        ["API key", changes.api_key_changed],
        ["Ingest URL", changes.ingest_url_changed],
      ];
      break;
    case "webhook":
      fields = [
        ["HMAC signing secret", changes.hmac_secret_changed],
        ["Webhook URL", changes.url_changed],
      ];
      break;
    case "sentry":
      fields = [["Sentry DSN", changes.dsn_changed]];
      break;
    case "postHogLogs":
    case "postHogErrorTracking":
      fields = [
        ["API key", changes.api_key_changed],
        ["PostHog host URL", changes.host_changed],
      ];
      break;
    case "s3Export":
      fields = [
        ["AWS access key ID", changes.access_key_id_changed],
        ["AWS secret access key", changes.secret_access_key_changed],
        ["AWS role ARN", changes.role_arn_changed ?? false],
      ];
      break;
    default:
      changes satisfies never;
      return [];
  }
  return fields
    .filter(([, changed]) => changed)
    .map(([label]) => `${label} changed`);
}

export function IntegrationAuditDetails({
  config,
  previous,
  credentialChanges,
}: {
  config?: Config;
  previous?: Config;
  credentialChanges?: CredentialChanges;
}) {
  if (!config && !credentialChanges) return null;
  const fields = config ? configurationFields(config) : [];
  const previousFields = previous ? configurationFields(previous) : [];
  const details = configurationDetails(config);
  const previousDetails = configurationDetails(previous);
  const detailsChanged =
    previousDetails &&
    details &&
    JSON.stringify(previousDetails.value) !== JSON.stringify(details.value);

  return (
    <div className="flex flex-col gap-2">
      {config && (
        <dl className="grid grid-cols-[auto_1fr] gap-x-3 gap-y-1">
          {fields.map(({ label, value }) => {
            const before = previousFields.find(
              (field) => field.label === label,
            )?.value;
            if (previous && before === value) return null;
            return (
              <div key={label} className="contents">
                <dt className="text-content-secondary">{label}</dt>
                <dd className="min-w-0 wrap-break-word">
                  {previous && (
                    <>
                      <span>{before ?? "(not configured)"}</span> →{" "}
                    </>
                  )}
                  <span>{value}</span>
                </dd>
              </div>
            );
          })}
        </dl>
      )}
      {sensitiveChanges(credentialChanges).map((change) => (
        <p key={change}>{change}</p>
      ))}
      {details && (
        <Disclosure>
          {({ open }) => (
            <>
              <DisclosureButton className="flex items-center gap-1 text-left text-content-secondary">
                {details.label}
                {detailsChanged ? " changed" : ""}
                {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
              </DisclosureButton>
              <DisclosurePanel>
                {detailsChanged && (
                  <div className="mt-2">
                    <p>
                      Previous{" "}
                      {details.label.toLowerCase().replace("export ", "")}
                    </p>
                    <pre className="max-h-60 overflow-auto text-xs">
                      {JSON.stringify(previousDetails.value, null, 2)}
                    </pre>
                  </div>
                )}
                <div className="mt-2">
                  {detailsChanged && (
                    <p>
                      Current{" "}
                      {details.label.toLowerCase().replace("export ", "")}
                    </p>
                  )}
                  <pre className="max-h-60 overflow-auto text-xs">
                    {JSON.stringify(details.value, null, 2)}
                  </pre>
                </div>
              </DisclosurePanel>
            </>
          )}
        </Disclosure>
      )}
    </div>
  );
}
