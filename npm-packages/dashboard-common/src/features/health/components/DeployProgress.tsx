import { ReactNode, useContext, useId, useState } from "react";
import { useQuery } from "convex/react";
import { FunctionReturnType } from "convex/server";
import {
  CheckCircledIcon,
  ChevronDownIcon,
  CrossCircledIcon,
  CubeIcon,
  TableIcon,
} from "@radix-ui/react-icons";
import udfs from "@common/udfs";
import { IndexIcon } from "@common/elements/icons";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { formatNumberCompact } from "@common/lib/format";
import { ProgressBar } from "@ui/ProgressBar";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";

export type SchemaValidationByTable = NonNullable<
  FunctionReturnType<typeof udfs.getSchemas.schemaValidationProgressByTable>
>;
export type BackfillingIndexes = FunctionReturnType<
  typeof udfs.indexes.backfilling
>;

// Rows shown before the list collapses behind "View more".
const MAX_ROWS = 4;

export function useDeployProgress(enabled: boolean) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canQuery = useIsOperationAllowed("ViewData") && enabled;
  const schema = useQuery(
    udfs.getSchemas.schemaValidationProgressByTable,
    canQuery ? { componentId: null } : "skip",
  );
  const indexes = useQuery(udfs.indexes.backfilling, canQuery ? {} : "skip");
  return {
    schema: schema ?? null,
    indexes: indexes ?? [],
    active: !!schema || (indexes !== undefined && indexes.length > 0),
  };
}

// Counters come from a different snapshot than the one being walked, so a
// finished walk can read slightly over 100%. Cap short of full until the
// document flips to a terminal state.
function fraction(done: number, total: number | null) {
  return total === null || total === 0
    ? undefined
    : Math.min(0.99, done / total);
}

function countLabel(done: number, total: number | null) {
  if (total === null) {
    return `${formatNumberCompact(done)} documents`;
  }
  const percent = Math.round((fraction(done, total) ?? 0) * 100);
  return `${percent}% · ${formatNumberCompact(done)} / ${formatNumberCompact(
    total,
  )} documents`;
}

/// A one-line status for the summary card. Its progress bar and per-table
/// detail unfold inline underneath while hovered or focused.
export function SchemaValidationStatus({
  schema,
  initiallyOpen,
}: {
  schema: SchemaValidationByTable;
  initiallyOpen?: boolean;
}) {
  // Tables still being walked come first so the visible rows are the ones
  // that are actually moving.
  const order = { pending: 0, failed: 1, valid: 2 };
  const tables = [...schema.tables].sort(
    (a, b) =>
      order[a.state] - order[b.state] || a.tableName.localeCompare(b.tableName),
  );
  return (
    <HoverDisclosure
      icon={<CubeIcon className="size-4 shrink-0 text-content-secondary" />}
      summary="Schema validation in progress"
      done={schema.numDocsValidated}
      total={schema.totalDocs}
      detailsLabel="Schema validation progress by table"
      initiallyOpen={initiallyOpen}
    >
      <ExpandableList
        rows={tables.map((table) => ({
          key: `${table.tableName}:${table.staged ? "staged" : "enforced"}`,
          node: <TableRow table={table} />,
        }))}
        noun="tables"
      />
    </HoverDisclosure>
  );
}

export function IndexCreationStatus({
  indexes,
  initiallyOpen,
}: {
  indexes: BackfillingIndexes;
  initiallyOpen?: boolean;
}) {
  // Backfills that haven't counted their table yet stay out of both sides of
  // the fraction until they report, so it reflects what is known without
  // overstating progress. With no denominator at all, show the raw count.
  const counted = indexes.filter(
    (index) => index.stats !== null && index.stats.totalDocs !== null,
  );
  const measured = counted.length > 0 ? counted : indexes;
  const done = measured.reduce(
    (sum, index) => sum + (index.stats?.numDocsIndexed ?? 0),
    0,
  );
  const total =
    counted.length > 0
      ? counted.reduce((sum, index) => sum + (index.stats?.totalDocs ?? 0), 0)
      : null;
  return (
    <HoverDisclosure
      icon={
        <IndexIcon kind="database" className="size-4 text-content-secondary" />
      }
      summary="Index creation in progress"
      done={done}
      total={total}
      detailsLabel="Index creation progress by index"
      initiallyOpen={initiallyOpen}
    >
      <ExpandableList
        rows={indexes.map((index) => ({
          key: `${index.tableName}.${index.name}`,
          node: (
            <ProgressRow
              label={
                <span className="flex min-w-0 items-center gap-1.5 text-sm">
                  <IndexIcon
                    kind={index.kind}
                    className="size-3.5 text-content-secondary"
                  />
                  <span className="truncate font-mono text-xs">
                    {index.tableName}.{index.name}
                  </span>
                  {index.staged && (
                    <span className="rounded-sm border px-1 text-xs text-content-secondary">
                      staged
                    </span>
                  )}
                </span>
              }
            >
              <PercentLabel
                done={index.stats?.numDocsIndexed ?? 0}
                total={index.stats?.totalDocs ?? null}
              />
            </ProgressRow>
          ),
        }))}
        noun="indexes"
      />
    </HoverDisclosure>
  );
}

function TableRow({
  table,
}: {
  table: SchemaValidationByTable["tables"][number];
}) {
  const label = (
    <span className="flex min-w-0 items-center gap-1.5 text-sm">
      <TableIcon className="size-3.5 shrink-0 text-content-secondary" />
      <span className="truncate font-mono text-xs">{table.tableName}</span>
      {table.staged && (
        <span className="rounded-sm border px-1 text-xs text-content-secondary">
          staged
        </span>
      )}
    </span>
  );
  switch (table.state) {
    case "valid":
      return (
        <ProgressRow label={label}>
          <span className="flex items-center gap-1 text-xs text-content-success">
            <CheckCircledIcon /> Valid
          </span>
        </ProgressRow>
      );
    case "failed":
      return (
        <ProgressRow label={label}>
          <Tooltip tip={table.error}>
            <span className="flex items-center gap-1 text-xs text-content-error">
              <CrossCircledIcon /> Validation failed
            </span>
          </Tooltip>
        </ProgressRow>
      );
    default:
      return (
        <ProgressRow label={label}>
          <PercentLabel done={table.numDocsValidated} total={table.totalDocs} />
        </ProgressRow>
      );
  }
}

// Inline disclosure instead of a popover: the detail is part of the card,
// pushes the content below it, and needs no dismissal. Hover or focus
// reveals it; a click pins it open for touch and keyboard readers. The
// summary line carries the job's one progress bar (indeterminate until the
// total is known), and the lines share a width so their chevrons align.
function HoverDisclosure({
  icon,
  summary,
  done,
  total,
  detailsLabel,
  initiallyOpen = false,
  children,
}: {
  icon: ReactNode;
  summary: string;
  done: number;
  total: number | null;
  detailsLabel: string;
  initiallyOpen?: boolean;
  children: ReactNode;
}) {
  const [hovered, setHovered] = useState(false);
  const [pinned, setPinned] = useState(initiallyOpen);
  const open = hovered || pinned;
  const detailsId = useId();
  const value = fraction(done, total);
  return (
    <div
      className="flex flex-col"
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
      onFocus={() => setHovered(true)}
      onBlur={(e) => {
        if (!e.currentTarget.contains(e.relatedTarget)) {
          setHovered(false);
        }
      }}
    >
      <Button
        variant="unstyled"
        onClick={() => setPinned((value) => !value)}
        aria-expanded={open}
        aria-controls={detailsId}
        // Hover already opens the details, so the pinned state needs its own
        // cue for the click to visibly register. The negative margin keeps
        // the padded highlight aligned with the lines around it, and half
        // opacity keeps the progress bar's track (the same token) visible.
        className={cn(
          "-mx-1 -my-0.5 flex w-[calc(var(--container-md)+0.5rem)] max-w-[calc(100%+0.5rem)] items-center gap-2 rounded-sm px-1 py-0.5 text-left transition-colors",
          pinned && "bg-background-tertiary/50",
        )}
      >
        {icon}
        <span className="min-w-0 flex-1 truncate text-sm text-content-primary">
          {summary}
        </span>
        <ProgressBar
          fraction={value}
          ariaLabel={summary}
          className="h-2.5 w-28 shrink-0"
        />
        <span className="w-[4ch] shrink-0 text-right text-xs text-content-secondary tabular-nums">
          {value === undefined ? "" : `${Math.round(value * 100)}%`}
        </span>
        <ChevronDownIcon
          className={cn(
            "size-3.5 text-content-tertiary transition-transform",
            open && "rotate-180",
          )}
        />
      </Button>
      <div
        id={detailsId}
        role="region"
        aria-label={detailsLabel}
        // Collapsed content stays mounted for the height transition, so it
        // must also leave the tab order.
        aria-hidden={!open}
        inert={!open}
        className={cn(
          "grid transition-[grid-template-rows] duration-200 ease-out",
          open ? "grid-rows-[1fr]" : "grid-rows-[0fr]",
        )}
      >
        <div className="min-w-0 overflow-hidden">
          <div className="flex w-md max-w-full flex-col gap-2 pt-2 pb-1 pl-6">
            <span className="self-end text-xs text-content-secondary tabular-nums">
              {countLabel(done, total)}
            </span>
            {children}
          </div>
        </div>
      </div>
    </div>
  );
}

// A row whose table hasn't been counted yet reads as 0% rather than showing a
// bare document count next to its neighbors' percentages.
function PercentLabel({ done, total }: { done: number; total: number | null }) {
  const value = fraction(done, total) ?? 0;
  return (
    <span className="text-xs text-content-secondary tabular-nums">
      {Math.round(value * 100)}%
    </span>
  );
}

function ProgressRow({
  label,
  children,
}: {
  label: ReactNode;
  children: ReactNode;
}) {
  return (
    <div className="flex items-center justify-between gap-3">
      {label}
      <div className="shrink-0">{children}</div>
    </div>
  );
}

// Long lists start collapsed to `MAX_ROWS`; "View more" reveals the rest
// inside a fixed-height inner scroll area so the card keeps its footprint.
function ExpandableList({
  rows,
  noun,
}: {
  rows: Array<{ key: string; node: ReactNode }>;
  noun: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const collapsible = rows.length > MAX_ROWS;
  const shown = collapsible && !expanded ? rows.slice(0, MAX_ROWS) : rows;
  return (
    <div className="flex flex-col gap-1.5">
      <div
        className={
          expanded
            ? "scrollbar flex scrollbar-gutter-stable flex-col gap-1.5 overflow-y-auto pr-1"
            : "flex flex-col gap-1.5"
        }
        style={expanded ? { maxHeight: "7.5rem" } : undefined}
      >
        {shown.map((row) => (
          <div key={row.key}>{row.node}</div>
        ))}
      </div>
      {collapsible && !expanded && (
        <Button
          variant="unstyled"
          onClick={() => setExpanded(true)}
          aria-label={`Show all ${noun}`}
          className="self-start text-[11px] text-content-secondary underline hover:text-content-primary"
        >
          View more
        </Button>
      )}
    </div>
  );
}
