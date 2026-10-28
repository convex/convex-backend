import { Insight } from "api/insights";
import { Sheet } from "@ui/Sheet";
import { Loading } from "@ui/Loading";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { useFunctionUrl } from "@common/lib/deploymentApi";
import Link from "next/link";
import { useNents } from "@common/lib/useNents";
import { ProblemForInsight } from "./ProblemForInsight";
import { ChartForInsight } from "./ChartForInsight";
import { EventsForInsight } from "./EventsForInsight";

export function InsightSummaryBreakdown({
  insight,
}: {
  insight?: Insight | null;
}) {
  const { nents } = useNents();
  const selectedNentId = nents?.find(
    (nent) => nent.path === insight?.componentPath,
  )?.id;
  const urlToSelectedFunction = useFunctionUrl(
    insight?.functionId || "",
    selectedNentId,
  );
  if (!insight) {
    return (
      <Sheet className="flex size-full items-center justify-center text-content-secondary">
        {insight === null ? (
          "Insight not found."
        ) : (
          <Loading className="w-full" />
        )}
      </Sheet>
    );
  }

  return (
    <Sheet className="scrollbar flex max-h-full min-h-160 max-w-full flex-col gap-4 overflow-y-auto">
      <h4 className="flex items-center gap-1">
        Insight Breakdown for{" "}
        <Link
          href={urlToSelectedFunction}
          className="font-semibold text-content-primary hover:underline"
        >
          <FunctionNameOption
            label={functionIdentifierValue(
              insight.functionId,
              insight.componentPath ?? undefined,
            )}
            oneLine
            disableTruncation
          />
        </Link>
      </h4>
      <div className="flex items-end justify-between gap-4">
        <ProblemForInsight insight={insight} explain />
      </div>
      <ChartForInsight insight={insight} />
      <EventsForInsight insight={insight} />
    </Sheet>
  );
}
