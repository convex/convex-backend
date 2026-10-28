import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import {
  CaretDownIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { Insight } from "api/insights";
import { Button } from "@ui/Button";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { Loading } from "@ui/Loading";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { formatBytes, formatNumberCompact } from "@common/lib/format";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { ComponentId, useNents } from "@common/lib/useNents";
import { documentHref } from "@common/lib/utils";
import { cn } from "@ui/cn";
import { Link } from "@ui/Link";
import { useContext } from "react";

// Type definitions to match what's in api/insights.ts
type HourlyCount = {
  hour: string;
  count: number;
};

type OccRecentEvent = {
  timestamp: string;
  id: string;
  request_id: string;
  occ_document_id: string;
  occ_write_source: string;
  occ_retry_count: number;
};

type BytesReadRecentEvent = {
  timestamp: string;
  id: string;
  request_id: string;
  calls: { table_name: string; bytes_read: number; documents_read: number }[];
  success: boolean;
};

// Type definitions for the event data
type FormattedBytesReadEvent = {
  timestamp: string;
  id: string;
  requestId: string;
  executionId: string;
  totalCount: number;
  events: { tableName: string; count: number }[];
  status: string;
};

type FormattedOccEvent = {
  timestamp: string;
  id: string;
  requestId: string;
  executionId: string;
  occDocumentId: string;
  occWriteSource: string;
  occRetryCount: number;
};

// Note: Hourly data padding and sorting is handled in the useInsights hook in api/insights.ts

// Type guards
function isOccInsight(insight: Insight): insight is Insight & {
  kind: "occRetried" | "occFailedPermanently";
  details: {
    occCalls: number;
    occTableName: string;
    hourlyCounts: HourlyCount[];
    recentEvents: OccRecentEvent[];
  };
} {
  return (
    insight.kind === "occRetried" || insight.kind === "occFailedPermanently"
  );
}

function isMetricsInsight(insight: Insight): insight is Insight & {
  kind:
    | "bytesReadLimit"
    | "bytesReadThreshold"
    | "documentsReadLimit"
    | "documentsReadThreshold";
  details: {
    count: number;
    hourlyCounts: HourlyCount[];
    recentEvents: BytesReadRecentEvent[];
  };
} {
  return [
    "bytesReadLimit",
    "bytesReadThreshold",
    "documentsReadLimit",
    "documentsReadThreshold",
  ].includes(insight.kind);
}

export function EventsForInsight({ insight }: { insight: Insight }) {
  return (
    <div className="flex flex-col gap-2 overflow-y-hidden">
      <div className="flex items-end justify-between">
        <div className="flex items-center gap-1">
          <h5>Recent Events</h5>
          <Tooltip
            tip="Recent events matching the criteria of this Insight."
            side="right"
          >
            <QuestionMarkCircledIcon />
          </Tooltip>
        </div>
        <span className="text-xs text-content-secondary">
          Data may be behind by a couple hours.
        </span>
      </div>
      {(() => {
        switch (insight.kind) {
          case "occFailedPermanently":
            return (
              <OCCFailedEvents
                insight={insight as Insight & { kind: "occFailedPermanently" }}
              />
            );
          case "occRetried":
            return (
              <OCCRetriedEvents
                insight={insight as Insight & { kind: "occRetried" }}
              />
            );
          case "bytesReadLimit":
          case "bytesReadThreshold":
            return (
              <BytesReadEvents
                insight={
                  insight as Insight & {
                    kind: "bytesReadLimit" | "bytesReadThreshold";
                  }
                }
              />
            );
          case "documentsReadLimit":
          case "documentsReadThreshold":
            return (
              <DocumentsReadEvents
                insight={
                  insight as Insight & {
                    kind: "documentsReadLimit" | "documentsReadThreshold";
                  }
                }
              />
            );
          default: {
            insight satisfies never;
            return null;
          }
        }
      })()}
    </div>
  );
}

const eventTimestampColumn = {
  header: (
    <div className="flex items-center gap-1">
      Timestamp{" "}
      <Tooltip tip="Events are sorted in reverse cronological order.">
        <CaretDownIcon />
      </Tooltip>
    </div>
  ),
  className: "w-40",
  key: "timestamp",
  Column: EventTimestamp,
};
const eventFunctionCallColumn = {
  header: "Function Call ID",
  className: "w-32",
  key: "executionId",
  tooltip: "The unique ID of the function call that encountered this error.",
  Column: EventExecutionId,
};
const eventRequestIdColumn = {
  header: "Request ID",
  className: "w-36",
  key: "requestId",
  tooltip:
    "The ID of the request that triggered this function call. Multiple function calls may have the same Request ID if they are part of the same request, or if a function was retried.",
  Column: EventRequestId,
};
const eventOccWriteSourceColumn = {
  header: "Conflicting Function",
  className: "w-60",
  key: "occWriteSource",
  tooltip: "The function that caused the write conflict.",
  Column: EventOccWriteSource,
};
const eventOccDocumentIdColumn = {
  header: "Conflicting Document ID",
  className: "w-[16rem]",
  key: "occDocumentId",
  tooltip: "The ID of the document that caused the write conflict.",
  Column: EventOccDocumentId,
};
const eventStatusColumn = {
  header: "Status",
  className: "w-16",
  key: "status",
  tooltip: "Whether the function call ultimately succeeded or failed.",
  Column: EventStatus,
};

function BytesReadEvents({
  insight,
}: {
  insight: Insight & {
    kind: "bytesReadLimit" | "bytesReadThreshold";
  };
}) {
  const events = insight.details.recentEvents.map((event) => {
    // Map BytesReadRecentEvent to FormattedBytesReadEvent
    const totalCount = event.calls.reduce(
      (sum, call) => sum + call.bytes_read,
      0,
    );
    return {
      timestamp: event.timestamp,
      id: event.id,
      requestId: event.request_id,
      executionId: event.id, // Using id as executionId
      totalCount,
      events: event.calls.map((call) => ({
        tableName: call.table_name,
        count: call.bytes_read,
      })),
      status: event.success ? "success" : "failure",
    } as FormattedBytesReadEvent;
  });

  // Hourly counts are now pre-processed in useInsights

  return (
    <EventsTable
      events={events}
      insight={insight}
      columns={[
        eventRequestIdColumn,
        eventFunctionCallColumn,
        eventTimestampColumn,
        eventStatusColumn,
        {
          header: "Bytes Read",
          className: "w-60",
          key: "bytesRead",
          tooltip: "The number of bytes read by the function during this call.",
          Column: BytesEventReadAmount,
        },
      ]}
    />
  );
}

function DocumentsReadEvents({
  insight,
}: {
  insight: Insight & {
    kind: "documentsReadLimit" | "documentsReadThreshold";
  };
}) {
  const events = insight.details.recentEvents.map((event) => {
    // Map BytesReadRecentEvent to FormattedBytesReadEvent but with documents_read
    const totalCount = event.calls.reduce(
      (sum, call) => sum + call.documents_read,
      0,
    );
    return {
      timestamp: event.timestamp,
      id: event.id,
      requestId: event.request_id,
      executionId: event.id, // Using id as executionId
      totalCount,
      events: event.calls.map((call) => ({
        tableName: call.table_name,
        count: call.documents_read,
      })),
      status: event.success ? "success" : "failure",
    } as FormattedBytesReadEvent;
  });

  // Hourly counts are now pre-processed in useInsights

  return (
    <EventsTable
      events={events}
      insight={insight}
      columns={[
        eventRequestIdColumn,
        eventFunctionCallColumn,
        eventTimestampColumn,
        eventStatusColumn,
        {
          header: "Documents Read",
          className: "w-60",
          key: "bytesRead",
          tooltip:
            "The number of documents read by the function during this call.",
          Column: DocumentsEventReadAmount,
        },
      ]}
    />
  );
}

function OCCFailedEvents({
  insight,
}: {
  insight: Insight & {
    kind: "occFailedPermanently";
  };
}) {
  const events = insight.details.recentEvents.map(
    (event) =>
      ({
        timestamp: event.timestamp,
        id: event.id,
        requestId: event.request_id,
        executionId: event.id, // Using id as executionId
        occDocumentId: event.occ_document_id,
        occWriteSource: event.occ_write_source,
        occRetryCount: event.occ_retry_count,
      }) as FormattedOccEvent,
  );

  // Hourly counts are now pre-processed in useInsights

  return (
    <EventsTable
      events={events}
      insight={insight}
      columns={[
        eventRequestIdColumn,
        eventFunctionCallColumn,
        eventTimestampColumn,
        eventOccDocumentIdColumn,
        eventOccWriteSourceColumn,
      ]}
    />
  );
}

function OCCRetriedEvents({
  insight,
}: {
  insight: Insight & {
    kind: "occRetried";
  };
}) {
  const events = insight.details.recentEvents.map(
    (event) =>
      ({
        timestamp: event.timestamp,
        id: event.id,
        requestId: event.request_id,
        executionId: event.id, // Using id as executionId
        occDocumentId: event.occ_document_id,
        occWriteSource: event.occ_write_source,
        occRetryCount: event.occ_retry_count,
      }) as FormattedOccEvent,
  );

  // Hourly counts are now pre-processed in useInsights

  return (
    <EventsTable
      events={events}
      insight={insight}
      columns={[
        eventRequestIdColumn,
        eventFunctionCallColumn,
        eventTimestampColumn,
        eventOccDocumentIdColumn,
        eventOccWriteSourceColumn,
        {
          header: "Retry #",
          className: "flex w-16 items-center gap-1",
          key: "occRetryCount",
          tooltip:
            "The number of previous attempts before this one to call the function.",
          Column: EventOccRetryCount,
        },
      ]}
    />
  );
}

/**
 * EventsTable is a generic table for displaying events.
 * It takes in a list of events and a list of columns to display.
 * Each column has a header, a className, a key, a tooltip, and a Column component.
 * The Column component is a React component that takes in an event and returns a JSX element.
 * The EventsTable component maps over the events and columns to render the table.
 * The table is scrollable and has a sticky header.
 */
function EventsTable<T, I extends Insight>({
  events,
  insight,
  columns,
}: {
  events?: T[];
  insight: I;
  columns: {
    header: React.ReactNode;
    className: string;
    key: string;
    tooltip?: string;
    Column: React.FC<{
      event: T;
      insight: I;
      componentId: ComponentId | undefined;
    }>;
  }[];
}) {
  const { nents } = useNents();
  const nentId = nents?.find((nent) => nent.path === insight.componentPath)?.id;
  return (
    <div className="scrollbar flex max-h-full w-full flex-col overflow-y-auto rounded-sm border">
      <div className="sticky top-0 z-20 flex min-w-fit gap-2 border-b bg-background-secondary px-2 pt-2 pb-1 text-xs text-content-secondary">
        {columns.map((col) => (
          <div
            key={col.key}
            className={cn("flex items-center gap-1", col.className)}
          >
            {col.header}
            {col.tooltip && (
              <Tooltip tip={col.tooltip}>
                <QuestionMarkCircledIcon />
              </Tooltip>
            )}
          </div>
        ))}
      </div>
      <div className="flex max-h-full grow flex-col">
        {events &&
          events.map((event, idx) => (
            <div
              key={idx}
              className="flex min-w-fit animate-fadeInFromLoading gap-2 border-b p-2 text-xs last:border-b-0"
            >
              {columns.map((col) => (
                <col.Column
                  key={col.key}
                  event={event}
                  insight={insight}
                  componentId={nentId}
                />
              ))}
            </div>
          ))}
        {!events &&
          Array.from(
            {
              length: Math.min(
                7,
                isOccInsight(insight)
                  ? insight.details.occCalls
                  : isMetricsInsight(insight)
                    ? insight.details.count
                    : 7,
              ),
            },
            (_, i) => i,
          ).map((i) => (
            <div
              key={i}
              className="flex w-full gap-2 border-b p-2 last:border-b-0"
            >
              {columns.map((col) => (
                <Loading className="h-4" key={col.key}>
                  <div className={col.className} />
                </Loading>
              ))}
            </div>
          ))}
      </div>
    </div>
  );
}

function EventTimestamp({ event }: { event: { timestamp: string } }) {
  return (
    <div className="min-w-40">
      {(() => {
        try {
          // Handle ISO strings with or without the Z/timezone information
          const timestamp = event.timestamp.endsWith("Z")
            ? event.timestamp
            : `${event.timestamp}Z`;
          return new Date(timestamp).toLocaleString();
        } catch {
          // Fallback to more forgiving date parsing
          return new Date(event.timestamp).toLocaleString();
        }
      })()}
    </div>
  );
}

function EventExecutionId({ event }: { event: { executionId: string } }) {
  return <div className="w-32 truncate">{event.executionId}</div>;
}

function EventRequestId({
  event,
  insight: _insight,
  componentId: _componentId,
}: {
  event: { requestId: string };
  insight: Insight;
  componentId: ComponentId | undefined;
}) {
  return (
    <div className="w-36">
      {event.requestId || (
        <span className="text-content-secondary">Unknown</span>
      )}
    </div>
  );
}

function EventOccDocumentId({
  insight,
  event,
  componentId,
}: {
  insight: Insight & {
    kind: "occFailedPermanently" | "occRetried";
  };
  event: FormattedOccEvent;
  componentId: ComponentId | undefined;
}) {
  const { deploymentsURI, captureMessage } = useContext(DeploymentInfoContext);
  return (
    <div className="flex w-[16rem]">
      {event.occDocumentId && insight.details.occTableName ? (
        <Link
          href={documentHref({
            deploymentsURI,
            tableName: insight.details.occTableName,
            id: event.occDocumentId,
            componentId: componentId ?? null,
            captureMessage,
          })}
          target="_blank"
          externalIcon
        >
          {event.occDocumentId}
        </Link>
      ) : (
        <span className="text-content-secondary">Unknown</span>
      )}
    </div>
  );
}

function EventOccWriteSource({
  insight,
  event,
  componentId: _componentId,
}: {
  insight: Insight & {
    kind: "occFailedPermanently" | "occRetried";
  };
  event: FormattedOccEvent;
  componentId: ComponentId | undefined;
}) {
  return (
    <div className="w-60 truncate">
      {!event.occWriteSource && (
        <span className="text-content-secondary">Unknown</span>
      )}
      {event.occWriteSource &&
        (insight.functionId === event.occWriteSource ? (
          <span className="flex items-center gap-1 text-content-secondary">
            Self{" "}
            <HelpTooltip>
              Two calls to the same function resulted in this write conflict.
            </HelpTooltip>
          </span>
        ) : (
          <FunctionNameOption
            label={functionIdentifierValue(event.occWriteSource)}
            oneLine
          />
        ))}
    </div>
  );
}

function EventOccRetryCount({
  event,
  insight: _insight,
  componentId: _componentId,
}: {
  event: FormattedOccEvent;
  insight: Insight & { kind: "occRetried" };
  componentId: ComponentId | undefined;
}) {
  return <div className="w-16">{event.occRetryCount}</div>;
}

function BytesEventReadAmount({
  event,
  insight: _insight,
  componentId: _componentId,
}: {
  event: FormattedBytesReadEvent;
  insight: Insight & { kind: "bytesReadLimit" | "bytesReadThreshold" };
  componentId: ComponentId | undefined;
}) {
  return <EventReadAmount event={event} format={formatBytes} />;
}

function DocumentsEventReadAmount({
  event,
  insight: _insight,
  componentId: _componentId,
}: {
  event: FormattedBytesReadEvent;
  insight: Insight & {
    kind: "documentsReadLimit" | "documentsReadThreshold";
  };
  componentId: ComponentId | undefined;
}) {
  return <EventReadAmount event={event} format={formatNumberCompact} />;
}

function EventReadAmount({
  event,
  format,
}: {
  event: FormattedBytesReadEvent;
  format: (count: number) => string;
}) {
  return (
    <div className="w-60">
      <Disclosure>
        {({ open }) => (
          <>
            <div className="flex items-center gap-1">
              <span className="min-w-17">{format(event.totalCount)}</span>
              <Tooltip tip="View breakdown" side="right" asChild>
                <DisclosureButton
                  as={Button}
                  inline
                  variant="neutral"
                  size="xs"
                  className="-my-1"
                  aria-label={open ? "Hide breakdown" : "Show breakdown"}
                >
                  {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
                </DisclosureButton>
              </Tooltip>
            </div>
            <DisclosurePanel>
              <ul className="mt-2 flex animate-fadeInFromLoading flex-col gap-1">
                {event.events.map((e, idx) => (
                  <li
                    key={idx}
                    className="list-inside list-decimal text-xs text-content-secondary"
                  >
                    <span className="font-semibold">{format(e.count)}</span>{" "}
                    from <span className="font-semibold">{e.tableName}</span>
                  </li>
                ))}
              </ul>
            </DisclosurePanel>
          </>
        )}
      </Disclosure>
    </div>
  );
}

function EventStatus({
  event,
  insight: _insight,
  componentId: _componentId,
}: {
  event: FormattedBytesReadEvent;
  insight: Insight & {
    kind:
      | "bytesReadLimit"
      | "bytesReadThreshold"
      | "documentsReadLimit"
      | "documentsReadThreshold";
  };
  componentId: ComponentId | undefined;
}) {
  return (
    <div className="w-16">
      {event.status === "success" ? (
        "Success"
      ) : event.status === "failure" ? (
        "Failure"
      ) : (
        <span className="text-content-secondary">Unknown</span>
      )}
    </div>
  );
}
