import { jsonToConvex, JSONValue } from "convex/values";
import { useRouter } from "next/router";
import { useMemo, useState } from "react";
import {
  CellContext,
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import { formatDuration } from "date-fns/formatDuration";
import { ChevronRightIcon, ExternalLinkIcon } from "@radix-ui/react-icons";
import {
  CronSchedule,
  CronJobLog,
  CronJobWithRuns,
} from "system-udfs/convex/_system/frontend/common";
import { useWasmCron } from "@common/features/schedules/lib/useWasmCron";
import {
  prettierSaffron,
  scheduleAsCron,
  scheduleLiteral,
} from "@common/features/schedules/lib/cronHelpers";
import { stringifyValue } from "@common/lib/stringifyValue";
import { formatExpression } from "@common/lib/format";
import { Tooltip } from "@ui/Tooltip";
import { useFunctionUrl } from "@common/lib/deploymentApi";
import { displayName } from "@common/lib/functions/generateFileTree";
import { Link } from "@ui/Link";
import { LiveTimestampDistance } from "@common/elements/TimestampDistance";
import { Button } from "@ui/Button";
import { DetailPanel } from "@common/elements/DetailPanel";
import { ReadonlyCode } from "@common/elements/ReadonlyCode";
import { Sheet } from "@ui/Sheet";
import { Doc } from "system-udfs/convex/_generated/dataModel";

// Shared by header and body cells, so every column needs a width that doesn't
// depend on its content for the header to line up with the rows.
const COLUMN_STYLES = [
  { flex: "2 0 80px" },
  { flex: "1 0 180px" },
  { flex: "2 2 60px" },
  { flex: "1 0 160px" },
  { flex: "0 0 7rem" },
  { flex: "0 0 2rem" },
];

function Name({ getValue }: CellContext<CronDatum, string>) {
  const value = getValue();
  return (
    <div title={value} className="text-sm font-medium">
      {value}
    </div>
  );
}

function Schedule({
  getValue,
}: CellContext<
  CronDatum,
  { schedule: CronSchedule; nextDate: Date | undefined }
>) {
  const { schedule, nextDate } = getValue();
  const literal = scheduleLiteral(schedule);

  let formattedSchedule = "";
  const wasmCron = useWasmCron();

  if (schedule.type === "interval") {
    const duration = formatDuration({ seconds: Number(schedule.seconds) });
    formattedSchedule = `Every ${duration}`;
  } else if (wasmCron) {
    // When the schedule omits the minute, Convex chooses it; describe the
    // schedule using the minute of the actual next run.
    const [cron, description] = wasmCron.parseAndDescribe(
      scheduleAsCron(schedule, nextDate?.getUTCMinutes()),
    );
    cron.free();
    formattedSchedule = prettierSaffron(description);
  }

  const tip = <pre className="text-left">{literal}</pre>;

  return (
    <div className="flex flex-col">
      <Tooltip tip={tip}>
        <div>{formattedSchedule}</div>
      </Tooltip>
    </div>
  );
}

function Function({ getValue }: CellContext<CronDatum, string>) {
  const value = getValue();
  const url = useFunctionUrl(value);
  const name = displayName(value);
  return (
    <Link href={url} className="truncate">
      {name}
    </Link>
  );
}

function PrevTs({
  date,
  run,
  isRunning,
}: {
  date?: Date;
  run?: CronJobLog;
  isRunning: boolean;
}) {
  if (!date || !run) return null;
  const message = `${
    isRunning
      ? "Started"
      : run.status.type === "success"
        ? "Success"
        : run.status.type === "err"
          ? "Failure"
          : "Run skipped"
  } `;
  return (
    <div className="flex flex-row truncate">
      <LiveTimestampDistance date={date} prefix={message} className="ml-1" />
    </div>
  );
}

function NextTs({ value }: { value: Date }) {
  return (
    <div className="flex flex-row truncate">
      <LiveTimestampDistance
        date={value}
        prefix={value < new Date() ? "Skipped run " : "Next run "}
        className="ml-1"
      />
    </div>
  );
}

function PrevNextTs({
  getValue,
}: CellContext<
  CronDatum,
  {
    nextDate: Date | undefined;
    prevDate: Date | undefined;
    prevRun: CronJobLog | undefined;
    nextRun: Doc<"_cron_next_run">;
  }
>) {
  const value = getValue();
  const isRunning = value.nextRun.state.type === "inProgress";
  return (
    <div className="flex flex-col truncate">
      <PrevTs date={value.prevDate} isRunning={isRunning} run={value.prevRun} />
      {value.nextDate && <NextTs value={value.nextDate} />}
    </div>
  );
}

function More({ getValue }: CellContext<CronDatum, string>) {
  const value = getValue();
  const router = useRouter();
  const handleClick = () => {
    router.query.id = value;
    void router.push({ query: router.query });
  };
  return (
    <Button
      onClick={handleClick}
      aria-label="show details"
      size="sm"
      variant="neutral"
      inline
      icon={<ChevronRightIcon aria-hidden />}
    />
  );
}

function Args({ getValue }: CellContext<CronDatum, JSONValue[]>) {
  const value = getValue();
  const [showArgs, setShowArgs] = useState(false);

  if (value.length === 0) {
    return <div className="h-6" />;
  }

  const args = value.map((arg) => jsonToConvex(arg));
  // Cron jobs almost always take a single argument object; show it unwrapped.
  const code = formatExpression(
    args.length === 1
      ? stringifyValue(args[0])
      : `[${args.map((arg) => stringifyValue(arg)).join(",")}]`,
  );

  return (
    <>
      <Button
        variant="neutral"
        inline
        size="sm"
        onClick={() => setShowArgs(true)}
        icon={<ExternalLinkIcon />}
      >
        Arguments
      </Button>
      {showArgs && (
        <DetailPanel
          onClose={() => setShowArgs(false)}
          header="Cron job arguments"
          content={
            <div className="h-full rounded-sm p-4">
              <ReadonlyCode path="scheduling" code={code} />
            </div>
          }
        />
      )}
    </>
  );
}

function cronDatum(cronJob: CronJobWithRuns) {
  const { name, cronSpec, lastRun, nextRun } = cronJob;
  const nextDate = new Date(Number(nextRun.nextTs / BigInt("1000000")));
  const prevDate = lastRun && new Date(Number(lastRun.ts / BigInt("1000000")));
  return {
    name,
    schedule: { schedule: cronSpec.cronSchedule, nextDate },
    prevNextTs: {
      prevDate: prevDate ?? undefined,
      nextDate,
      prevRun: lastRun ?? undefined,
      nextRun,
    },
    udfPath: cronSpec.udfPath,
    udfArgs:
      cronSpec.udfArgs &&
      (JSON.parse(new TextDecoder().decode(cronSpec.udfArgs)) as JSONValue[]),
  };
}
type CronDatum = ReturnType<typeof cronDatum>;

const columnHelper = createColumnHelper<CronDatum>();

export function CronsTable({ cronJobs }: { cronJobs: CronJobWithRuns[] }) {
  const columns = useMemo(
    () => [
      columnHelper.accessor("name", { header: "Name", cell: Name }),
      columnHelper.accessor("schedule", { header: "Schedule", cell: Schedule }),
      columnHelper.accessor("udfPath", { header: "Function", cell: Function }),
      columnHelper.accessor("prevNextTs", {
        header: "Next/Last Run",
        cell: PrevNextTs,
      }),
      columnHelper.accessor("udfArgs", { header: "Args", cell: Args }),
      columnHelper.accessor("name", {
        id: "more",
        header: () => <span className="sr-only">More</span>,
        cell: More,
      }),
    ],
    [],
  );

  const data = useMemo(() => cronJobs.map(cronDatum), [cronJobs]);

  const table = useReactTable({
    columns,
    data,
    getCoreRowModel: getCoreRowModel(),
  });

  return (
    <Sheet padding={false} className="scrollbar overflow-x-auto">
      <div role="table" className="block min-w-2xl">
        <div role="rowgroup" className="border-b">
          {table.getHeaderGroups().map((headerGroup) => (
            <div
              key={headerGroup.id}
              role="row"
              className="flex gap-2 px-4 py-2 text-xs text-content-secondary"
            >
              {headerGroup.headers.map((header, i) => (
                <div
                  key={header.id}
                  role="columnheader"
                  style={COLUMN_STYLES[i]}
                  className="overflow-hidden text-left"
                >
                  {flexRender(
                    header.column.columnDef.header,
                    header.getContext(),
                  )}
                </div>
              ))}
            </div>
          ))}
        </div>
        <div role="rowgroup" className="divide-y">
          {table.getRowModel().rows.map((row) => (
            <div
              key={row.id}
              role="row"
              className="flex items-stretch justify-start gap-2 px-4 py-3 text-xs text-content-primary"
            >
              {row.getVisibleCells().map((cell, i) => (
                <div
                  key={cell.id}
                  role="cell"
                  style={COLUMN_STYLES[i]}
                  className="flex items-center overflow-hidden"
                >
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>
    </Sheet>
  );
}
