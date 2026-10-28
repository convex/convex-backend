import React, {
  useState,
  useCallback,
  useMemo,
  useContext,
  createContext,
  PropsWithChildren,
} from "react";
import { cn } from "@ui/cn";
import { ChevronLeftIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { MultiSelectCombobox, MultiSelectValue } from "@ui/MultiSelectCombobox";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { HealthView } from "@common/features/health/components/HealthView";
import {
  Insight,
  useInsights,
  useInsightsPeriod,
  getInsightPageIdentifier,
} from "api/insights";
import { useCurrentDeployment, useDeploymentRegions } from "api/deployments";
import { useCurrentTeam, useTeamMembers } from "api/teams";
import { useCurrentProject } from "api/projects";
import { useListCloudBackupsIfAvailable } from "api/backups";
import { PermissionsContext } from "@common/lib/deploymentContext";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { deploymentResource } from "lib/permissions";
import { useRouter } from "next/router";
import { Link } from "@ui/Link";
import {
  itemIdentifier,
  useModuleFunctions,
} from "@common/lib/functions/FunctionsProvider";
import {
  functionIdentifierValue,
  functionIdentifierFromValue,
} from "@common/lib/functions/generateFileTree";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { SmallInsightsSummary } from "./SmallInsightsSummary";
import { InsightsSummary } from "./InsightsSummary";
import { InsightSummaryBreakdown } from "./InsightsSummaryBreakdown";
import { UrlObject } from "url";

// We need a context here so the insights components can have data provided to them without rerendering the Health page.
const InsightsContext = createContext<
  | {
      page: string;
      insights?: Insight[];
      selectedFunctions: MultiSelectValue;
      setSelectedFunctions: (selectedFunctions: MultiSelectValue) => void;
      canViewInsights: boolean;
    }
  | undefined
>(undefined);

export function HealthWithInsights() {
  const { query, push } = useRouter();
  const page = (query.view as string)?.startsWith("insight:")
    ? (query.view as string)
    : query.view === "insights"
      ? "insights"
      : "home";
  const moduleFunctions = useModuleFunctions();
  const functions = useMemo(
    () => [
      ...moduleFunctions.map((value) => itemIdentifier(value)),
      functionIdentifierValue("_other"),
    ],
    [moduleFunctions],
  );
  const [selectedFunctions, setSelectedFunctions] =
    useState<MultiSelectValue>("all");

  const insights = useInsights();
  const { from } = useInsightsPeriod();

  // Get deployment info for summary
  const deployment = useCurrentDeployment();
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const teamMembers = useTeamMembers(team?.id);
  const { regions } = useDeploymentRegions(team?.id);

  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewBackups = useIsOperationAllowed("ViewBackups");
  const backups = useListCloudBackupsIfAvailable(
    canViewBackups ? deployment : undefined,
  );

  // Get the most recent backup for this deployment
  // backups is null when not available (d1024, non-cloud), undefined when loading
  const lastBackupTime = useMemo(() => {
    if (backups === null) return null;
    if (backups === undefined) return undefined;
    const deploymentsBackups = backups.filter((b) => b.state === "complete");
    return deploymentsBackups.length > 0
      ? deploymentsBackups[0].requestedTime
      : null;
  }, [backups]);

  const insightsResource =
    project && deployment && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  const isAdmin = useHasProjectAdminPermissions(project?.id);
  const canViewInsightsCustom = useHasCustomRolePermission(
    team?.id,
    "deployment:insights:view",
    insightsResource,
    true,
  );
  const canViewInsights = isAdmin || canViewInsightsCustom !== false;

  const selectedInsight = insights?.find(
    (insight) => getInsightPageIdentifier(insight) === page,
  );

  const header = (
    <div
      className={cn(
        "sticky top-0 mx-6 flex flex-wrap items-center justify-between gap-4 pt-2",
        page === "insights" ? "max-w-280" : "",
      )}
    >
      <div className="flex items-center gap-2">
        {canViewInsights && page.startsWith("insight") && (
          <Button
            icon={<ChevronLeftIcon className="size-5" />}
            tip="Back to Health"
            onClick={() =>
              void push(
                {
                  pathname: "/t/[team]/[project]/[deploymentName]",
                  query: {
                    team: query.team,
                    project: query.project,
                    deploymentName: query.deploymentName,
                    ...(query.lowInsightsThreshold
                      ? { lowInsightsThreshold: query.lowInsightsThreshold }
                      : {}),
                  },
                },
                undefined,
                { shallow: true },
              )
            }
            size="xs"
            variant="neutral"
            className="text-content-secondary hover:text-content-primary"
            inline
          />
        )}
        <h3 className="flex items-center gap-2 py-2">
          <MaybeLink
            href={
              page === "home"
                ? null
                : {
                    pathname: "/t/[team]/[project]/[deploymentName]",
                    query: {
                      team: query.team,
                      project: query.project,
                      deploymentName: query.deploymentName,
                      ...(query.lowInsightsThreshold
                        ? { lowInsightsThreshold: query.lowInsightsThreshold }
                        : {}),
                    },
                  }
            }
          >
            Health
          </MaybeLink>{" "}
          {canViewInsights && page.startsWith("insight") && (
            <>
              <span className="animate-fadeInFromLoading">/</span>
              <span className="animate-fadeInFromLoading">
                <MaybeLink
                  href={
                    page === "insights"
                      ? null
                      : {
                          pathname: "/t/[team]/[project]/[deploymentName]",
                          query: {
                            team: query.team,
                            project: query.project,
                            deploymentName: query.deploymentName,
                            view: "insights",
                            ...(query.lowInsightsThreshold
                              ? {
                                  lowInsightsThreshold:
                                    query.lowInsightsThreshold,
                                }
                              : {}),
                          },
                        }
                  }
                >
                  Insights
                </MaybeLink>
              </span>
            </>
          )}
          {canViewInsights && selectedInsight && (
            <>
              <span className="animate-fadeInFromLoading">/</span>
              <div className="flex animate-fadeInFromLoading flex-wrap gap-1 text-content-primary">
                Insight Breakdown{" "}
              </div>
            </>
          )}
        </h3>
      </div>
      {canViewInsights && page === "insights" && (
        <div className="flex animate-fadeInFromLoading flex-wrap items-center gap-4">
          <span className="text-sm text-content-secondary">
            {new Date(from).toLocaleString([], {
              year: "numeric",
              month: "numeric",
              day: "numeric",
              hour: "numeric",
              minute: undefined,
            })}{" "}
            – Now
          </span>
          <div className="min-w-[20rem]">
            <MultiSelectCombobox
              options={functions}
              selectedOptions={selectedFunctions}
              setSelectedOptions={setSelectedFunctions}
              unit="function"
              unitPlural="functions"
              label="Functions"
              labelHidden
              Option={FunctionNameOption}
              processFilterOption={(option) => {
                const id = functionIdentifierFromValue(option);
                return id.componentPath
                  ? `${id.componentPath}/${id.identifier}`
                  : id.identifier;
              }}
            />
          </div>
        </div>
      )}
    </div>
  );

  const providerValue = useMemo(
    () => ({
      page,
      insights: canViewInsights ? insights : undefined,
      selectedFunctions,
      setSelectedFunctions,
      canViewInsights,
    }),
    [page, insights, selectedFunctions, setSelectedFunctions, canViewInsights],
  );

  return (
    <InsightsContext.Provider value={providerValue}>
      <HealthView
        header={header}
        PagesWrapper={InsightsWrapper}
        PageWrapper={PageWrapper}
        deployment={deployment}
        teamSlug={team?.slug}
        projectSlug={project?.slug}
        lastBackupTime={lastBackupTime}
        teamMembers={teamMembers}
        regions={regions}
      />
    </InsightsContext.Provider>
  );
}

function InsightsWrapper({ children }: { children: React.ReactNode }) {
  const { insights, selectedFunctions, page, canViewInsights } =
    useContext(InsightsContext) || {};
  return (
    <div
      className={cn(
        "flex min-h-0 grow gap-6 transition-transform duration-500 motion-reduce:transition-none",
        page === "home" && "translate-x-0",
        page === "insights" && "-translate-x-[calc(100%+1.5rem)]",
        page?.startsWith("insight:") && "-translate-x-[calc(200%+3rem)]",
      )}
    >
      {children}
      <div
        // @ts-expect-error https://github.com/facebook/react/issues/17157
        inert={page !== "insights" ? "inert" : undefined}
        className="mb-6 flex w-full shrink-0 px-6"
      >
        {canViewInsights === false ? (
          <Sheet className="scrollbar h-fit max-h-full w-full max-w-280 min-w-0 overflow-auto py-12">
            <NoPermissionMessage
              message="You do not have permission to view insights."
              missingPermission="deployment:insights:view"
            />
          </Sheet>
        ) : (
          <Sheet
            padding={false}
            className="scrollbar h-fit max-h-full w-full max-w-280 min-w-0 overflow-auto"
          >
            <InsightsSummary
              insights={insights?.filter((insight) => {
                if (!selectedFunctions) return true;
                if (selectedFunctions === "all") return true;

                return (
                  selectedFunctions.includes(
                    functionIdentifierValue(
                      insight.functionId,
                      insight.componentPath ?? undefined,
                    ),
                  ) || selectedFunctions.includes("_other")
                );
              })}
            />
          </Sheet>
        )}
      </div>
      <div
        // @ts-expect-error https://github.com/facebook/react/issues/17157
        inert={!page.startsWith("insight:") ? "inert" : undefined}
        className="scrollbar flex w-full shrink-0 overflow-y-auto px-6"
      >
        {canViewInsights === false ? (
          <Sheet className="max-w-3xl py-12">
            <NoPermissionMessage
              message="You do not have permission to view insights."
              missingPermission="deployment:insights:view"
            />
          </Sheet>
        ) : (
          <InsightSummaryBreakdown
            insight={
              insights
                ? insights?.find(
                    (insight) => getInsightPageIdentifier(insight) === page,
                  ) || null
                : undefined
            }
          />
        )}
      </div>
    </div>
  );
}

function PageWrapper({ children }: { children: React.ReactNode }) {
  const { query, push } = useRouter();
  const onViewAll = useCallback(() => {
    void push(
      {
        pathname: "/t/[team]/[project]/[deploymentName]",
        query: {
          team: query.team,
          project: query.project,
          deploymentName: query.deploymentName,
          view: "insights",
          ...(query.lowInsightsThreshold
            ? { lowInsightsThreshold: query.lowInsightsThreshold }
            : {}),
        },
      },
      undefined,
      { shallow: true },
    );
  }, [query, push]);
  const { page, canViewInsights } = useContext(InsightsContext) || {};
  return (
    <div
      className="scrollbar flex w-full shrink-0 grow flex-col overflow-y-auto px-6 pb-4"
      // @ts-expect-error https://github.com/facebook/react/issues/17157
      inert={page !== "home" ? "inert" : undefined}
    >
      {children}
      <SmallInsightsSummary
        onViewAll={onViewAll || (() => {})}
        canViewInsights={canViewInsights}
      />
    </div>
  );
}

function MaybeLink({
  href,
  children,
}: PropsWithChildren<{
  href: UrlObject | null;
}>) {
  return href === null ? (
    <span>{children}</span>
  ) : (
    <Link href={href}>{children}</Link>
  );
}
