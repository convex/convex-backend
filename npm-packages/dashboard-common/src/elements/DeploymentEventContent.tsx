import { IntegrationAuditDetails } from "./IntegrationAuditDetails";
import {
  ChevronUpIcon,
  ChevronDownIcon,
  TrashIcon,
  PlusIcon,
  UpdateIcon,
} from "@radix-ui/react-icons";
import { useContext, useMemo } from "react";
import { Infer } from "convex/values";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import {
  authDiff,
  componentDiff,
  cronDiffType,
  schemaDiffType,
  usageLimitConfig,
} from "system-udfs/convex/tableDefs/deploymentAuditLogTable";
import {
  METRIC_CONFIG,
  WINDOW_SUFFIX,
  LIMIT_TYPE_LABEL,
  formatAmount,
  type UsageMetric,
  type UsageLimitWindow,
  type UsageLimitType,
} from "@common/features/settings/components/UsageLimits";
import { SchemaJson, displaySchema } from "@common/lib/format";
import { DeploymentAuditLogEvent } from "@common/lib/useDeploymentAuditLog";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { Button } from "@ui/Button";
import { ReadonlyCodeDiff } from "@common/elements/ReadonlyCode";
import { NentNameOption } from "@common/elements/NentSwitcher";
import { NENT_APP_PLACEHOLDER } from "@common/lib/useNents";
import { Tooltip } from "@ui/Tooltip";
import { CopyButton } from "@common/elements/CopyButton";
import { cn } from "@ui/cn";

function useSchemaCode(schema: null | string): string {
  return useMemo(() => {
    if (!schema) return "";
    const schema_json: SchemaJson = JSON.parse(schema);
    return displaySchema(schema_json) ?? "";
  }, [schema]);
}

export function DeploymentEventContent({
  event,
  inPanel = false,
}: {
  event: DeploymentAuditLogEvent;
  inPanel?: boolean;
}) {
  const { TeamMemberLink } = useContext(DeploymentInfoContext);
  // Usage limit breaches and the resulting deployment state changes aren't
  // attributable to a member, so the action text reads as a standalone
  // sentence rather than "<actor> did X".
  const hasActor = ![
    "usage_limit_exceeded",
    "change_usage_limit_stop_state",
  ].includes(event.action);
  let body;
  switch (event.action) {
    case "build_indexes":
      // There are old audit log events for building indexes that
      // are redundant with the schema diff in push config events.
      return null;

    case "push_config":
      body = <PushContent event={event} />;
      break;

    case "push_config_with_components":
      body = (
        <>
          {event.metadata.message && (
            <p className="mb-2 text-content-primary italic">
              &ldquo;{event.metadata.message}&rdquo;
            </p>
          )}
          <NodeVersionChange
            nodeVersionDiff={event.metadata.node_version_diff ?? null}
          />
          {event.metadata.component_diffs.map(
            ({ component_path, component_diff }) => {
              const auth =
                component_path === null ? event.metadata.auth_diff : undefined;
              return (
                <PushContentForComponents
                  key={component_path}
                  diff={component_diff}
                  componentName={component_path}
                  auth={auth}
                />
              );
            },
          )}
        </>
      );
      break;

    case "snapshot_import":
      body = <SnapshotImportContent event={event} />;
      break;

    case "update_usage_limit":
      body = UsageLimitUpdateBody({ event });
      break;

    case "create_integration":
    case "delete_integration":
      body = event.metadata.config ? (
        <IntegrationAuditDetails config={event.metadata.config} />
      ) : null;
      break;

    case "update_integration":
      body =
        event.metadata.current || event.metadata.credential_changes ? (
          <IntegrationAuditDetails
            config={event.metadata.current}
            previous={event.metadata.previous}
            credentialChanges={event.metadata.credential_changes}
          />
        ) : null;
      break;

    case "create_environment_variable":
    case "delete_environment_variable":
    case "update_environment_variable":
    case "replace_environment_variable":
    case "update_canonical_url":
    case "delete_canonical_url":
    case "change_deployment_state":
    case "pause_deployment":
    case "unpause_deployment":
    case "change_usage_limit_stop_state":
    case "change_system_stop_state":
    case "clear_tables":
    case "delete_scheduled_jobs_table":
    case "delete_tables":
    case "delete_component":
    case "cancel_all_scheduled_functions":
    case "cancel_scheduled_function":
    case "request_export":
    case "download_export":
    case "cancel_export":
    case "set_export_expiration":
    case "add_documents":
    case "delete_documents":
    case "update_documents":
    case "create_table":
    case "delete_files":
    case "generate_upload_url":
    case "create_data_sync":
    default:
      body = null;
  }

  return (
    <div className="flex flex-col gap-2 text-sm">
      <div className="flex items-center justify-between">
        <span className={cn(!inPanel && "leading-6")}>
          {hasActor && (
            <>
              <TeamMemberLink
                memberId={Number(event.member_id)}
                name={event.memberName}
              />{" "}
            </>
          )}
          <ActionText event={event} />
        </span>
        {!inPanel && <TimestampDistance date={new Date(event._creationTime)} />}
      </div>
      {body && <div className="ml-4 rounded-md border px-3 py-2.5">{body}</div>}
    </div>
  );
}

export function ActionText({ event }: { event: DeploymentAuditLogEvent }) {
  const { CloudImport } = useContext(DeploymentInfoContext);
  switch (event.action) {
    case "create_environment_variable":
      return (
        <>
          <span>created the environment variable </span>
          <span className="font-mono font-semibold">
            {event.metadata.variable_name}
          </span>
        </>
      );
    case "delete_environment_variable":
      return (
        <>
          <span>deleted the environment variable </span>
          <span className="font-mono font-semibold">
            {event.metadata.variable_name}
          </span>
        </>
      );
    case "update_environment_variable":
      return (
        <>
          <span>updated the environment variable </span>
          <span className="font-mono font-semibold">
            {event.metadata.variable_name}
          </span>
        </>
      );

    case "replace_environment_variable":
      return (
        <>
          <span>renamed the environment variable from</span>
          <span className="font-mono font-semibold">
            {event.metadata.previous_variable_name}
          </span>
          <span> to </span>
          <span className="font-mono font-semibold">
            {event.metadata.variable_name}
          </span>
        </>
      );

    case "update_canonical_url":
      return (
        <>
          <span>updated the canonical URL for </span>
          <span className="font-mono font-semibold">
            {event.metadata.request_destination}
          </span>
          <span> to </span>
          <span className="font-mono font-semibold">{event.metadata.url}</span>
        </>
      );

    case "delete_canonical_url":
      return (
        <>
          <span>deleted the canonical URL for </span>
          <span className="font-mono font-semibold">
            {event.metadata.request_destination}
          </span>
        </>
      );

    case "build_indexes":
      return <span>updated indexes</span>;

    case "push_config":
      return <span>deployed functions</span>;

    case "push_config_with_components":
      return <span>deployed functions</span>;

    case "change_deployment_state":
      switch (event.metadata.new_state) {
        case "paused":
          return <span>paused the deployment</span>;
        case "running":
          return <span>resumed the deployment</span>;
        case "disabled":
          return <span>disabled the deployment</span>;
        case "suspended":
          return <span>suspended the deployment</span>;
        default:
          event.metadata.new_state satisfies never;
          return null;
      }

    case "pause_deployment":
      return <span>paused the deployment</span>;

    case "unpause_deployment":
      return <span>unpaused the deployment</span>;

    case "change_system_stop_state":
      switch (event.metadata.new_state) {
        case "suspended":
          return <span>suspended the deployment</span>;
        case "disabled":
          return <span>disabled the deployment</span>;
        case "none":
          return <span>unblocked the deployment</span>;
        default:
          event.metadata.new_state satisfies never;
          return null;
      }

    case "clear_tables":
      return <span>cleared tables</span>;

    case "snapshot_import": {
      if (event.metadata.requestor.type === "cloudRestore") {
        return (
          <CloudImport
            sourceCloudBackupId={Number(
              event.metadata.requestor.sourceCloudBackupId,
            )}
          />
        );
      }
      let format = "";
      switch (event.metadata.import_format.format) {
        case "csv":
          format = "CSV";
          break;
        case "jsonl":
          format = "JSONL";
          break;
        case "json_array":
          format = "JSON";
          break;
        case "zip":
          format = "ZIP";
          break;
        default:
          event.metadata.import_format satisfies never;
          return null;
      }
      return <span>imported a snapshot from a {format} file</span>;
    }

    case "delete_scheduled_jobs_table":
      return (
        <>
          <span>deleted the </span>
          <span className="font-mono font-semibold">_scheduled_functions</span>
          <span> table</span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "delete_tables": {
      const { table_names } = event.metadata;
      return (
        <>
          <span>deleted </span>
          {table_names.length === 1 ? (
            <span>
              table{" "}
              <span className="font-mono font-semibold">{table_names[0]}</span>
            </span>
          ) : (
            <ItemCount items={table_names} singular="table" plural="tables" />
          )}
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );
    }

    case "delete_component":
      return (
        <>
          <span>deleted component </span>
          {event.metadata.component ? (
            <ComponentName
              component={event.metadata.component}
              componentId={event.metadata.component_id}
            />
          ) : (
            <span className="font-mono font-semibold">
              {event.metadata.component_id}
            </span>
          )}
        </>
      );

    case "cancel_all_scheduled_functions":
      return (
        <>
          <span>canceled all scheduled functions</span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "cancel_scheduled_function":
      return (
        <>
          <span>canceled </span>
          <Tooltip
            tip={
              <span className="font-mono">
                {event.metadata.scheduled_function_id}
              </span>
            }
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">
              scheduled function
            </span>
          </Tooltip>
          {event.metadata.function_path && (
            <>
              <span> </span>
              <span className="font-mono font-semibold">
                {event.metadata.function_path}
              </span>
            </>
          )}
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "request_export":
      return (
        <>
          <span>requested a </span>
          <span className="font-mono font-semibold">
            {event.metadata.format}
          </span>
          <span> </span>
          <Tooltip
            tip={<span className="font-mono">{event.metadata.id}</span>}
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">export</span>
          </Tooltip>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "download_export":
      return (
        <>
          <span>downloaded a </span>
          <span className="font-mono font-semibold">
            {event.metadata.format}
          </span>
          <span> </span>
          <Tooltip
            tip={<span className="font-mono">{event.metadata.id}</span>}
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">export</span>
          </Tooltip>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "cancel_export":
      return (
        <>
          <span>canceled </span>
          <Tooltip
            tip={<span className="font-mono">{event.metadata.id}</span>}
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">an export</span>
          </Tooltip>
        </>
      );

    case "set_export_expiration":
      return (
        <>
          <span>set expiration on </span>
          <Tooltip
            tip={<span className="font-mono">{event.metadata.id}</span>}
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">an export</span>
          </Tooltip>
          <span>
            {" "}
            to{" "}
            {new Date(Number(event.metadata.expiration_ts_ms)).toLocaleString()}
          </span>
        </>
      );

    case "create_integration":
      return (
        <>
          <span>created a </span>
          <IntegrationRef type={event.metadata.type} id={event.metadata.id} />
          <span> integration</span>
        </>
      );

    case "update_integration":
      return (
        <>
          <span>updated a </span>
          <IntegrationRef type={event.metadata.type} id={event.metadata.id} />
          <span> integration</span>
        </>
      );

    case "delete_integration":
      return (
        <>
          <span>deleted a </span>
          <IntegrationRef type={event.metadata.type} id={event.metadata.id} />
          <span> integration</span>
        </>
      );

    case "add_documents": {
      const { document_ids } = event.metadata;
      return (
        <>
          <span>added </span>
          <ItemCount
            items={document_ids}
            singular="document"
            plural="documents"
          />
          <span> to </span>
          <span className="font-mono font-semibold">
            {event.metadata.table}
          </span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );
    }

    case "delete_documents": {
      const { document_ids, table } = event.metadata;
      return (
        <>
          <span>deleted </span>
          <ItemCount
            items={document_ids}
            singular="document"
            plural="documents"
          />
          <span> from </span>
          <span className="font-mono font-semibold">{table}</span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );
    }

    case "update_documents": {
      const { document_ids, table } = event.metadata;
      return (
        <>
          <span>updated </span>
          <ItemCount
            items={document_ids}
            singular="document"
            plural="documents"
          />
          <span> in </span>
          <span className="font-mono font-semibold">{table}</span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );
    }

    case "create_table":
      return (
        <>
          <span>created table </span>
          <span className="font-mono font-semibold">
            {event.metadata.table}
          </span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "delete_files": {
      const { storage_ids } = event.metadata;
      return (
        <>
          <span>deleted </span>
          <Tooltip
            tip={<ItemListTooltip items={storage_ids} />}
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">
              {storage_ids.length === 1
                ? "a file"
                : `${storage_ids.length} files`}
            </span>
          </Tooltip>
          <span> from storage</span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );
    }

    case "generate_upload_url":
      return (
        <>
          <span>generated a file upload URL</span>
          <ComponentSuffix
            component={event.metadata.component}
            componentId={event.metadata.component_id}
          />
        </>
      );

    case "create_data_sync":
      return (
        <>
          <span>started a </span>
          <Tooltip
            tip={<span className="font-mono">{event.metadata.sync_id}</span>}
            maxWidthClassName="max-w-md"
          >
            <span className="underline decoration-dotted">data sync</span>
          </Tooltip>
        </>
      );

    case "create_usage_limit":
      return (
        <>
          <span>created a usage limit: </span>
          <UsageLimitSummary config={event.metadata.config} />
        </>
      );

    case "delete_usage_limit":
      return (
        <>
          <span>deleted a usage limit: </span>
          <UsageLimitSummary config={event.metadata.config} />
        </>
      );

    case "usage_limit_exceeded":
      return (
        <>
          <span>A usage limit was exceeded: </span>
          <UsageLimitSummary
            config={event.metadata.config}
            showStatus={false}
          />
        </>
      );

    case "change_usage_limit_stop_state":
      return event.metadata.new_state === "disabled" ? (
        <span>The deployment was disabled after exceeding a usage limit</span>
      ) : (
        <span>
          The deployment was re-enabled after usage fell back under its limits
        </span>
      );

    case "update_usage_limit": {
      // The changed fields are shown as a diff in the event body; the sentence
      // just names which limit was updated.
      const { metricName, typeLabel } = usageLimitLabels(
        event.metadata.current,
      );
      return (
        <>
          <span>updated the </span>
          <span className="font-semibold">{typeLabel}</span>
          <span> on </span>
          <span className="font-semibold">{metricName}</span>
        </>
      );
    }

    default:
      event satisfies never;
      return null;
  }
}

function usageLimitLabels(config: Infer<typeof usageLimitConfig>) {
  const metricConfig = METRIC_CONFIG[config.metric as UsageMetric];
  return {
    metricName: metricConfig?.name ?? config.metric,
    unit: metricConfig?.rawUnitShort ?? "",
    windowSuffix: WINDOW_SUFFIX[config.window as UsageLimitWindow] ?? "",
    typeLabel:
      LIMIT_TYPE_LABEL[config.limitType as UsageLimitType] ?? config.limitType,
  };
}

function UsageLimitSummary({
  config,
  showStatus = true,
}: {
  config: Infer<typeof usageLimitConfig>;
  showStatus?: boolean;
}) {
  const { metricName, unit, windowSuffix, typeLabel } =
    usageLimitLabels(config);
  return (
    <>
      <span className="font-semibold">{typeLabel}</span>
      <span> on </span>
      <span className="font-semibold">{metricName}</span>
      <span>
        {" ("}
        {formatAmount(Number(config.limit))} {unit} {windowSuffix}
        {showStatus && (config.enabled ? ", enabled" : ", disabled")})
      </span>
    </>
  );
}

function UsageLimitUpdateBody({
  event,
}: {
  event: DeploymentAuditLogEvent & { action: "update_usage_limit" };
}) {
  const { previous, current } = event.metadata;
  const { unit, windowSuffix } = usageLimitLabels(current);
  const rows: { label: string; from: string; to: string }[] = [];
  if (previous.limit !== current.limit) {
    rows.push({
      label: "Limit",
      from: `${formatAmount(Number(previous.limit))} ${unit} ${windowSuffix}`,
      to: `${formatAmount(Number(current.limit))} ${unit} ${windowSuffix}`,
    });
  }
  if (previous.enabled !== current.enabled) {
    rows.push({
      label: "Status",
      from: previous.enabled ? "Enabled" : "Disabled",
      to: current.enabled ? "Enabled" : "Disabled",
    });
  }
  if (rows.length === 0) {
    return null;
  }
  return (
    <div className="flex flex-col gap-1 text-sm">
      {rows.map((row) => (
        <div key={row.label} className="flex items-center gap-1.5">
          <span className="text-content-secondary">{row.label}</span>
          <span className="font-mono">{row.from}</span>
          <span className="text-content-secondary">→</span>
          <span className="font-mono font-semibold">{row.to}</span>
        </div>
      ))}
    </div>
  );
}

function ItemListTooltip({ items }: { items: string[] }) {
  return (
    <div className="relative text-left">
      <div className="absolute top-0 right-0">
        <CopyButton text={items.join("\n")} size="xs" inline />
      </div>
      <div className="scrollbar flex max-h-48 flex-col gap-0.5 overflow-y-auto pr-8 font-mono">
        {items.map((item) => (
          <span key={item}>{item}</span>
        ))}
      </div>
    </div>
  );
}

function IntegrationRef({ type, id }: { type: string; id: string }) {
  return (
    <Tooltip
      tip={<span className="font-mono">{id}</span>}
      maxWidthClassName="max-w-md"
    >
      <span className="font-mono font-semibold underline decoration-dotted">
        {type}
      </span>
    </Tooltip>
  );
}

function ItemCount({
  items,
  singular,
  plural,
}: {
  items: string[];
  singular: string;
  plural: string;
}) {
  if (items.length === 1) {
    return (
      <Tooltip
        tip={<span className="font-mono">{items[0]}</span>}
        maxWidthClassName="max-w-md"
      >
        <span className="underline decoration-dotted">a {singular}</span>
      </Tooltip>
    );
  }
  return (
    <Tooltip
      tip={<ItemListTooltip items={items} />}
      maxWidthClassName="max-w-md"
    >
      <span className="underline decoration-dotted">
        {items.length} {plural}
      </span>
    </Tooltip>
  );
}

function ComponentName({
  component,
  componentId,
}: {
  component: string;
  componentId: string | null;
}) {
  if (componentId) {
    return (
      <Tooltip tip={<span className="font-mono">{componentId}</span>}>
        <span className="font-mono font-semibold underline decoration-dotted">
          {component}
        </span>
      </Tooltip>
    );
  }
  return <span className="font-mono font-semibold">{component}</span>;
}

function ComponentSuffix({
  component,
  componentId,
}: {
  component: string | null;
  componentId: string | null;
}) {
  if (component === null) {
    return null;
  }
  return (
    <>
      <span> in component </span>
      <ComponentName component={component} componentId={componentId} />
    </>
  );
}

function Variable({ variableName }: { variableName: string }) {
  return <div className="font-mono font-semibold">{variableName}</div>;
}

function CronElement({ cronDiff }: { cronDiff: Infer<typeof cronDiffType> }) {
  const hasCronDiff =
    cronDiff &&
    (cronDiff.added.length > 0 ||
      cronDiff.updated.length > 0 ||
      cronDiff.deleted.length > 0);
  if (!hasCronDiff) {
    return null;
  }
  const cronElement = (
    <div>
      {cronDiff.added.map((name) => (
        <div key={name} className="flex items-start gap-1.5">
          <Added />
          <span>cron job </span>
          <span className="font-mono font-semibold">{name}</span>
        </div>
      ))}
      {cronDiff.updated.map((name) => (
        <div key={name} className="flex items-start gap-1.5">
          <Updated />
          <span>cron job </span>
          <span className="font-mono font-semibold">{name}</span>
        </div>
      ))}
      {cronDiff.deleted.map((name) => (
        <div key={name} className="flex items-start gap-1.5">
          <Deleted />
          <span>cron job </span>
          <span className="font-mono font-semibold">{name}</span>
        </div>
      ))}
    </div>
  );
  return (
    <Disclosure>
      {({ open }) => (
        <>
          <div className="flex items-center gap-1.5">
            <span>Updated cron jobs</span>
            <DisclosureButton
              as={Button}
              inline
              variant="neutral"
              aria-label={open ? "Hide details" : "Show details"}
            >
              {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
            </DisclosureButton>
          </div>

          <DisclosurePanel className="ps-4">{cronElement}</DisclosurePanel>
        </>
      )}
    </Disclosure>
  );
}

type ServerVersion = { previous_version: string; next_version: string } | null;
function ServerVersionChange({
  serverVersion,
}: {
  serverVersion: ServerVersion;
}) {
  return serverVersion ? (
    <div className="flex items-center gap-1 text-sm">
      <span>Set the convex package version to</span>
      <Variable variableName={serverVersion.next_version} />
    </div>
  ) : null;
}

type NodeVersionDiff = {
  previous_version: string | null;
  next_version: string | null;
} | null;
function NodeVersionChange({
  nodeVersionDiff,
}: {
  nodeVersionDiff: NodeVersionDiff;
}) {
  if (!nodeVersionDiff) {
    return null;
  }
  const { previous_version, next_version } = nodeVersionDiff;

  if (next_version === null) {
    if (previous_version === null) {
      throw new Error(
        "Unexpected Node.js version transition from null to null",
      );
    }
    return (
      <div className="flex items-center gap-1 text-sm">
        <span>Updated the Node.js version from</span>
        <Variable variableName={previous_version} />
        <span>to using the default Node.js version</span>
      </div>
    );
  }
  if (previous_version === null) {
    return (
      <div className="flex items-center gap-1 text-sm">
        <span>Set the Node.js version to</span>
        <Variable variableName={next_version} />
      </div>
    );
  }
  return (
    <div className="flex items-center gap-1 text-sm">
      <span>Updated the Node.js version from</span>
      <Variable variableName={previous_version} />
      <span>to</span>
      <Variable variableName={next_version} />
    </div>
  );
}

function SchemaElement(diff: Infer<typeof schemaDiffType>) {
  const previousSchemaCode = useSchemaCode(diff?.previous_schema ?? null);
  const nextSchemaCode = useSchemaCode(diff?.next_schema ?? null);
  if (previousSchemaCode === nextSchemaCode) {
    return null;
  }
  return (
    <Disclosure>
      {({ open }) => (
        <>
          <div className="flex items-center gap-1.5">
            <span>Updated the schema</span>
            <DisclosureButton
              as={Button}
              inline
              variant="neutral"
              aria-label={open ? "Hide details" : "Show details"}
            >
              {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
            </DisclosureButton>
          </div>

          <Disclosure.Panel className="ps-4">
            <ReadonlyCodeDiff
              language="javascript"
              path="schema.js"
              height={{ type: "content", maxHeightRem: 30 }}
              originalCode={previousSchemaCode}
              modifiedCode={nextSchemaCode}
            />
          </Disclosure.Panel>
        </>
      )}
    </Disclosure>
  );
}

function AuthElement({ diff }: { diff: Infer<typeof authDiff> }) {
  const hasAuthDiff = diff.added.length > 0 || diff.removed.length > 0;
  if (!hasAuthDiff) {
    return null;
  }
  const authElement = (
    <>
      {diff.added.map((newAuth, idx) => (
        <div className="flex items-start gap-1.5" key={idx}>
          <Added />
          <span className="font-mono font-semibold">{newAuth}</span>
        </div>
      ))}
      {diff.removed.map((removedAuth, idx) => (
        <div className="flex items-start gap-1.5" key={idx}>
          <Deleted />
          <span className="font-mono font-semibold">{removedAuth}</span>
        </div>
      ))}
    </>
  );
  return (
    <Disclosure>
      {({ open }) => (
        <>
          <div className="flex items-center gap-1.5">
            <span>Updated auth providers</span>
            <DisclosureButton
              as={Button}
              inline
              variant="neutral"
              aria-label={open ? "Hide details" : "Show details"}
            >
              {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
            </DisclosureButton>
          </div>

          <Disclosure.Panel className="ps-4">{authElement}</Disclosure.Panel>
        </>
      )}
    </Disclosure>
  );
}

function PushContent({
  event,
}: {
  event: DeploymentAuditLogEvent & { action: "push_config" };
}) {
  const { auth, crons, server_version, schema } = event.metadata;
  const authElement = AuthElement({ diff: auth });

  const cronElement = <CronElement cronDiff={crons} />;

  const serverVersionChange = ServerVersionChange({
    serverVersion: server_version,
  });
  const schemaElement = SchemaElement(schema);
  return (
    <div className="flex flex-col gap-3 text-sm">
      <div className="flex items-center gap-1 text-sm">
        <UpdateIcon className="text-content-primary" />
        deployed functions
      </div>
      {serverVersionChange}
      {authElement}
      {cronElement}
      {schemaElement}
    </div>
  );
}

function PushContentForComponents({
  diff,
  auth,
  componentName,
}: {
  diff: Infer<typeof componentDiff>;
  auth?: Infer<typeof authDiff>;
  componentName: string | null;
}) {
  const { cronDiff, schemaDiff, udfConfigDiff, diffType } = diff;
  const cronElement = CronElement({ cronDiff });

  const serverVersionChange = ServerVersionChange({
    serverVersion: udfConfigDiff,
  });
  const schemaElement = SchemaElement(schemaDiff);
  let pastTenseDiff;
  switch (diffType.type) {
    case "create":
      pastTenseDiff = "Created";
      break;
    case "modify":
      pastTenseDiff = "Modified";
      break;
    case "unmount":
      pastTenseDiff = "Unmounted";
      break;
    case "remount":
      pastTenseDiff = "Remounted";
      break;
    default:
      diffType.type satisfies never;
  }
  const authElement = auth ? AuthElement({ diff: auth }) : null;
  return (
    <div dir="ltr">
      <NentNameOption label={componentName ?? NENT_APP_PLACEHOLDER} inButton />
      <div className="my-2 ms-8">
        <div className="flex flex-col gap-3 text-sm">
          <ul className="list-disc">
            {componentName ? (
              <li>{pastTenseDiff} component</li>
            ) : (
              <li>Updated functions</li>
            )}
            {authElement && <li>{authElement}</li>}
            {serverVersionChange && <li>{serverVersionChange}</li>}
            {cronElement && <li>{cronElement}</li>}
            {schemaElement && <li>{schemaElement}</li>}
          </ul>
        </div>
      </div>
    </div>
  );
}

function Added() {
  return (
    <div className="flex items-center gap-1 text-sm">
      <PlusIcon className="size-3 text-content-primary" />
      added
    </div>
  );
}

function Updated() {
  return (
    <div className="flex items-center gap-1 text-sm">
      <UpdateIcon className="text-content-primary" />
      updated
    </div>
  );
}

function Deleted() {
  return (
    <div className="flex items-center gap-1 text-sm">
      <TrashIcon className="size-3 text-content-primary" />
      deleted
    </div>
  );
}

type TableNamesInput = { table_names: string[]; component: string | null }[];
type TableNameObject = { table_name: string; component: string | null };

function transformTableNames(input: TableNamesInput): TableNameObject[] {
  // Handle array of { table_names, component } objects
  return input.flatMap((item) =>
    item.table_names.map((table_name) => ({
      table_name,
      component: item.component,
    })),
  );
}

function SnapshotImportContent({
  event,
}: {
  event: DeploymentAuditLogEvent & { action: "snapshot_import" };
}) {
  const table_names = transformTableNames(event.metadata.table_names);
  const table_names_deleted = transformTableNames(
    event.metadata.table_names_deleted,
  );
  const omittedTables = Number(event.metadata.table_count) - table_names.length;
  const omittedTablesDeleted =
    Number(event.metadata.table_count_deleted) - table_names_deleted.length;
  return (
    <div className="flex flex-col gap-3 text-sm">
      {table_names.map(({ table_name, component }) => (
        <SnapshotImportIntoTable
          key={`${table_name} ${component}`}
          table={table_name}
          component={component}
          import_mode={event.metadata.import_mode}
        />
      ))}
      {omittedTables > 0 ? <span>and {omittedTables} more</span> : null}
      {table_names_deleted.map(({ table_name, component }) => (
        <SnapshotImportIntoTable
          key={`${table_name} ${component}`}
          table={table_name}
          component={component}
          import_mode={event.metadata.import_mode}
          deleted
        />
      ))}
      {omittedTablesDeleted > 0 ? (
        <span>and {omittedTablesDeleted} more</span>
      ) : null}
    </div>
  );
}

function SnapshotImportIntoTable({
  table,
  component,
  import_mode,
  deleted,
}: {
  table: string;
  component: string | null;
  import_mode: "RequireEmpty" | "Append" | "Replace" | "ReplaceAll";
  deleted?: boolean;
}) {
  let icon = null;
  let action = "";
  switch (import_mode) {
    case "RequireEmpty":
      icon = <PlusIcon className="size-3 text-content-primary" />;
      action = "created";
      break;
    case "Append":
      icon = <PlusIcon className="size-3 text-content-primary" />;
      action = "appended to";
      break;
    case "Replace":
    case "ReplaceAll":
      icon = <UpdateIcon className="text-content-primary" />;
      action = "replaced";
      break;
    default:
      console.error(`Unexpected import_mode ${import_mode}`);
      import_mode satisfies never;
      return null;
  }

  if (deleted) {
    action = "deleted";
  }

  return (
    <div className="flex items-center gap-1 text-sm">
      {icon}
      {action}
      <Variable variableName={table} />
      {component !== null && (
        <>
          {" in component "}
          <Variable variableName={component} />
        </>
      )}
    </div>
  );
}
