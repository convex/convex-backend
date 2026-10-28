import { useCurrentDeployment } from "api/deployments";
import { useInsights, useInsightsPeriod } from "api/insights";
import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { DisclosureSection } from "@common/features/health/components/HealthView";
import {
  ChevronRightIcon,
  CrossCircledIcon,
  ExclamationTriangleIcon,
} from "@radix-ui/react-icons";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { InsightsSummary } from "./InsightsSummary";
import { severityForInsightKind } from "./InsightsSummaryListItem";

export function SmallInsightsSummary({
  onViewAll,
  canViewInsights,
}: {
  onViewAll: () => void;
  canViewInsights?: boolean;
}) {
  const deployment = useCurrentDeployment();
  const insights = useInsights();
  const { from } = useInsightsPeriod();

  if (deployment?.kind === "local") {
    return null;
  }

  const criticalCount =
    insights?.filter(
      (insight) => severityForInsightKind[insight.kind] === "error",
    ).length ?? 0;

  const warningCount =
    insights?.filter(
      (insight) => severityForInsightKind[insight.kind] === "warning",
    ).length ?? 0;

  // Don't show closedDescription until data has loaded
  const closedDescription = insights ? (
    <span className="flex animate-fadeInFromLoading items-center gap-3 text-xs text-content-secondary">
      {criticalCount > 0 && (
        <span className="flex items-center gap-1 text-content-error">
          <CrossCircledIcon className="size-3 min-w-3" />
          {criticalCount} critical
        </span>
      )}
      {warningCount > 0 && (
        <span className="flex items-center gap-1 text-content-warning">
          <ExclamationTriangleIcon className="size-3 min-w-3" />
          {warningCount} warning{warningCount !== 1 ? "s" : ""}
        </span>
      )}
      {criticalCount === 0 && warningCount === 0 && "No issues"}
    </span>
  ) : null;

  return (
    <DisclosureSection
      id="insights"
      title="Insights"
      defaultOpen
      closedDescription={canViewInsights === false ? null : closedDescription}
    >
      {canViewInsights === false ? (
        <Sheet className="flex w-full max-w-3xl animate-fadeInFromLoading flex-col py-12">
          <NoPermissionMessage
            message="You do not have permission to view insights."
            missingPermission="deployment:insights:view"
          />
        </Sheet>
      ) : (
        <>
          <Sheet
            padding={false}
            className="flex w-full animate-fadeInFromLoading flex-col"
          >
            <div className="flex items-center justify-between p-2">
              <span className="text-xs text-content-secondary">
                {new Date(from).toLocaleString([], {
                  year: "numeric",
                  month: "numeric",
                  day: "numeric",
                  hour: "numeric",
                  minute: undefined,
                })}{" "}
                – Now
              </span>
            </div>
            <InsightsSummary insights={insights?.slice(0, 5)} />
          </Sheet>
          <div className="flex">
            {insights && insights.length > 0 && (
              <Button
                variant="neutral"
                className="m-auto mt-2 w-fit gap-1 hover:bg-background-tertiary"
                onClick={onViewAll}
              >
                View{" "}
                {insights.length > 6
                  ? `${insights.length - 5} more insights`
                  : "all Insights"}{" "}
                <ChevronRightIcon />
              </Button>
            )}
          </div>
        </>
      )}
    </DisclosureSection>
  );
}
