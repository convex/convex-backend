import React, { JSX, useContext } from "react";
import { cn } from "@ui/cn";
import { ChevronDownIcon } from "@radix-ui/react-icons";
import { SchedulerStatus } from "@common/elements/SchedulerStatus";
import { FunctionCalls } from "@common/features/health/components/FunctionCalls";
import { FailureRate } from "@common/features/health/components/FailureRate";
import { CacheHitRate } from "@common/features/health/components/CacheHitRate";
import { DeploymentPageTitle } from "@common/elements/DeploymentPageTitle";
import { PageContent } from "@common/elements/PageContent";
import { useConcurrencyStatus } from "@common/features/health/components/ConcurrencyStatus";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { HealthCard } from "@common/elements/HealthCard";
import { ChartForFunctionRate } from "@common/features/health/components/ChartForFunctionRate";
import { DeploymentSummary } from "@common/features/health/components/DeploymentSummary";
import { SubscriptionInvalidations } from "@common/features/health/components/SubscriptionInvalidations";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { Sheet } from "@ui/Sheet";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";

export function HealthView({
  header,
  PageWrapper,
  PagesWrapper,
  deployment,
  teamSlug,
  projectSlug,
  lastBackupTime,
  teamMembers,
  regions,
}: {
  header: JSX.Element;
  PageWrapper: React.FC<{ children: React.ReactNode }>;
  PagesWrapper: React.FC<{ children: React.ReactNode }>;
  deployment?: PlatformDeploymentResponse;
  teamSlug?: string;
  projectSlug?: string;
  lastBackupTime?: number | null;
  teamMembers?: Array<{ id: number; name?: string | null; email: string }>;
  regions?: Array<{ name: string; displayName: string }>;
}) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewMetrics = useIsOperationAllowed("ViewMetrics");

  const {
    closedDescription: concurrencyClosedDescription,
    lag,
    running,
    queued,
  } = useConcurrencyStatus(canViewMetrics, 4);

  return (
    <PageContent>
      <DeploymentPageTitle title="Health" />
      <div className="flex max-h-full max-w-full flex-col overflow-hidden pt-2">
        {header}
        <PagesWrapper>
          <PageWrapper>
            <div>
              {deployment && teamSlug && projectSlug && (
                <DisclosureSection id="summary" title="Summary" defaultOpen>
                  <DeploymentSummary
                    deployment={deployment}
                    teamSlug={teamSlug}
                    projectSlug={projectSlug}
                    lastBackupTime={lastBackupTime}
                    teamMembers={teamMembers}
                    regions={regions}
                    showDeployProgress
                  />
                </DisclosureSection>
              )}

              <DisclosureSection
                id="function-calls"
                title="Functions"
                defaultOpen
                closedDescription={
                  <span className="text-xs text-content-secondary">
                    3 charts
                  </span>
                }
              >
                {canViewMetrics ? (
                  <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
                    <FunctionCalls />
                    <FailureRate />
                    <CacheHitRate />
                  </div>
                ) : (
                  <Sheet className="max-w-3xl py-12">
                    <NoPermissionMessage
                      message="You do not have permission to view metrics."
                      missingPermission="deployment:metrics:view"
                    />
                  </Sheet>
                )}
              </DisclosureSection>

              <DisclosureSection
                id="concurrency"
                title="Concurrency"
                defaultOpen={false}
                closedDescription={concurrencyClosedDescription}
              >
                {canViewMetrics ? (
                  <div className="grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-3">
                    <SchedulerStatus lag={lag} />
                    <HealthCard
                      title="Running Functions"
                      tip="The maximum number of concurrently running functions in a given minute. This includes system functions used to power the Convex Dashboard."
                    >
                      <ChartForFunctionRate
                        chartData={running}
                        kind="functionConcurrency"
                      />
                    </HealthCard>
                    <HealthCard
                      title="Queued Functions"
                      tip="The maximum number of functions waiting to be ran in a given minute. Functions are queued when the concurrency limit has been reached. If a function is queued for too long, it will discarded."
                    >
                      <ChartForFunctionRate
                        chartData={queued}
                        kind="functionConcurrency"
                      />
                    </HealthCard>
                    <SubscriptionInvalidations />
                  </div>
                ) : (
                  <Sheet className="max-w-3xl py-12">
                    <NoPermissionMessage
                      message="You do not have permission to view metrics."
                      missingPermission="deployment:metrics:view"
                    />
                  </Sheet>
                )}
              </DisclosureSection>
            </div>
          </PageWrapper>
        </PagesWrapper>
      </div>
    </PageContent>
  );
}

export function DisclosureSection({
  id,
  title,
  defaultOpen = true,
  children,
  closedDescription,
}: {
  id: string;
  title: string;
  defaultOpen?: boolean;
  children: React.ReactNode;
  closedDescription?: React.ReactNode;
}) {
  const storageKey = `health-disclosure-${id}`;
  const [isOpen, setIsOpen] = useGlobalLocalStorage(storageKey, defaultOpen);

  return (
    <div>
      <h4
        className={cn(
          "sticky top-0 z-10 bg-background-primary/70 backdrop-blur-[2px]",
          !isOpen && "border-b",
        )}
      >
        {/* eslint-disable-next-line react/forbid-elements -- Using native button for disclosure heading to avoid Button component styling */}
        <button
          type="button"
          onClick={() => setIsOpen(!isOpen)}
          className={cn(
            "-mx-2 my-1.5 flex flex-wrap items-center gap-2 rounded-full py-1.5 pl-2 text-left font-normal transition-colors hover:bg-background-tertiary",
            // The smaller closed description reads as cramped against the round
            // cap with the same padding the title gets.
            isOpen ? "pr-2" : "pr-3",
          )}
        >
          <ChevronDownIcon
            className={cn(
              "size-4 text-content-secondary transition-transform",
              isOpen && "rotate-180",
            )}
          />
          <span className="font-semibold text-content-primary">{title} </span>
          {!isOpen && <span className="font-normal">{closedDescription}</span>}
        </button>
      </h4>
      {isOpen && <div>{children}</div>}
    </div>
  );
}
