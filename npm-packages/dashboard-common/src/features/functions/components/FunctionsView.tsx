import { useContext, useState } from "react";
import { ArrowLeftIcon, CodeIcon } from "@radix-ui/react-icons";
import {
  useCurrentOpenFunction,
  useModuleFunctions,
} from "@common/lib/functions/FunctionsProvider";
import { DirectorySidebar } from "@common/features/functions/components/DirectorySidebar";
import { FunctionSummary } from "@common/features/functions/components/FunctionSummary";
import { PerformanceGraphs } from "@common/features/functions/components/PerformanceGraphs";
import { SingleGraph } from "@common/features/functions/components/SingleGraph";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { SidebarDetailLayout } from "@common/layouts/SidebarDetailLayout";
import { EmptySection } from "@common/elements/EmptySection";
import { DeploymentPageTitle } from "@common/elements/DeploymentPageTitle";
import { NoPermissionMessage } from "@common/elements/NoPermissionMessage";
import { Tab } from "@ui/Tab";
import {
  TabGroup as HeadlessTabGroup,
  TabPanel as HeadlessTabPanel,
  TabPanels as HeadlessTabPanels,
} from "@headlessui/react";
import { useNents } from "@common/lib/useNents";
import { Sheet } from "@ui/Sheet";
import { FunctionLogs } from "./FunctionLogs";

export function FunctionsView() {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");

  if (!canViewData) {
    return (
      <>
        <DeploymentPageTitle title="Functions" />
        <NoPermissionMessage
          message="You do not have permission to view functions in this deployment."
          missingPermission="deployment:data:view"
        />
      </>
    );
  }

  return (
    <>
      <DeploymentPageTitle title="Functions" />
      <Functions />
    </>
  );
}
function Functions() {
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();
  const deploymentId = deployment && "id" in deployment ? deployment.id : null;
  const currentOpenFunction = useCurrentOpenFunction();
  const modules = useModuleFunctions();
  const [selectedTabIndex, setSelectedTabIndex] = useState(0);
  const { selectedNent } = useNents();

  if (modules.length === 0) {
    return <EmptyFunctions />;
  }

  let content: React.ReactNode;
  if (!currentOpenFunction) {
    content = (
      <div className="flex size-full items-center justify-center">
        <Sheet className="m-8 flex size-fit max-w-[48ch] items-center gap-4 text-balance">
          <ArrowLeftIcon
            className="size-8 min-w-8 text-content-primary"
            aria-hidden
          />
          Select a function in the expandable panel to the left to view metrics
          and logs.
        </Sheet>
      </div>
    );
  } else {
    content = (
      <div className="flex h-full min-w-0 grow flex-col overflow-hidden">
        <HeadlessTabGroup
          selectedIndex={selectedTabIndex}
          onChange={setSelectedTabIndex}
          className="flex min-h-0 grow flex-col"
        >
          <div className="sticky top-0 z-10 scrollbar shrink-0 overflow-x-auto overflow-y-hidden bg-background-secondary">
            <div className="flex-none px-6 pt-4">
              <FunctionSummary currentOpenFunction={currentOpenFunction} />
            </div>
            <div className="-ml-2 flex gap-2 border-b px-6">
              <Tab>Statistics</Tab>
              <Tab>Logs</Tab>
            </div>
          </div>

          <HeadlessTabPanels className="scrollbar flex min-h-0 w-full max-w-440 min-w-0 grow flex-col overflow-auto p-6">
            <HeadlessTabPanel className="grow">
              <PerformanceGraphs />
            </HeadlessTabPanel>

            <HeadlessTabPanel className="flex min-h-0 min-w-0 grow">
              <FunctionLogs
                key={currentOpenFunction.displayName}
                currentOpenFunction={currentOpenFunction}
                selectedNent={selectedNent || undefined}
              />
            </HeadlessTabPanel>
          </HeadlessTabPanels>
        </HeadlessTabGroup>
      </div>
    );
  }

  return (
    <SidebarDetailLayout
      resizeHandleTitle="Functions"
      panelSizeKey={`${deploymentId}/functions`}
      sidebarComponent={<DirectorySidebar onChangeFunction={() => {}} />}
      contentComponent={content}
    />
  );
}

export function EmptyFunctions() {
  return (
    <div className="relative size-full animate-fadeIn overflow-hidden">
      {/* Background example */}
      <div
        className="pointer-events-none absolute inset-0 select-none"
        style={{
          maskImage:
            "linear-gradient(to bottom, rgba(0,0,0,0.6) 0%, rgb(0,0,0,0.3) 30%, transparent 85%)",
        }}
        inert
      >
        <div className="flex size-full flex-col">
          {/* Example Function Summary */}
          <div className="sticky top-0 z-10 mb-6 shrink-0 bg-background-secondary">
            <div className="flex-none px-6 pt-4">
              <div className="flex items-center gap-4">
                <div className="flex flex-col gap-1">
                  <div className="flex items-center gap-2">
                    <span className="font-mono text-sm font-semibold text-content-secondary">
                      example/function:myFunction
                    </span>
                    <span className="rounded-sm bg-yellow-500/10 p-1 text-xs font-semibold text-yellow-500">
                      Query
                    </span>
                  </div>
                </div>
              </div>
            </div>
            <div className="-ml-2 flex gap-2 border-b px-6">
              <div className="border-b-2 border-content-primary px-3 py-2 text-sm">
                Statistics
              </div>
              <div className="px-3 py-2 text-sm text-content-secondary">
                Logs
              </div>
            </div>
          </div>

          {/* Example Performance Graphs */}
          <div className="px-6">
            <div
              className="grid gap-2"
              style={{
                gridTemplateColumns: "repeat(auto-fit, minmax(24rem, 1fr))",
              }}
            >
              <SingleGraph
                title="Function Calls"
                data={{
                  data: [
                    { time: "12:00 PM", metric: 42 },
                    { time: "12:15 PM", metric: 43 },
                    { time: "12:30 PM", metric: 44 },
                    { time: "12:45 PM", metric: 45 },
                    { time: "1:00 PM", metric: 46 },
                    { time: "1:15 PM", metric: 47 },
                    { time: "1:30 PM", metric: 46 },
                    { time: "1:45 PM", metric: 47 },
                    { time: "2:00 PM", metric: 47 },
                    { time: "2:15 PM", metric: 46 },
                    { time: "2:30 PM", metric: 45 },
                    { time: "2:45 PM", metric: 44 },
                    { time: "3:00 PM", metric: 45 },
                    { time: "3:15 PM", metric: 46 },
                    { time: "3:30 PM", metric: 45 },
                    { time: "3:45 PM", metric: 44 },
                    { time: "4:00 PM", metric: 45 },
                  ],
                  xAxisKey: "time",
                  lineKeys: [
                    {
                      key: "metric",
                      name: "calls",
                      color: "var(--chart-line-1)",
                    },
                  ],
                }}
                syncId="fnMetrics"
              />
              <SingleGraph
                title="Errors"
                data={{
                  data: [
                    { time: "12:00 PM", metric: 0 },
                    { time: "12:15 PM", metric: 0 },
                    { time: "12:30 PM", metric: 2 },
                    { time: "12:45 PM", metric: 0 },
                    { time: "1:00 PM", metric: 0 },
                    { time: "1:15 PM", metric: 0 },
                    { time: "1:30 PM", metric: 0 },
                    { time: "1:45 PM", metric: 0 },
                    { time: "2:00 PM", metric: 0 },
                    { time: "2:15 PM", metric: 0 },
                    { time: "2:30 PM", metric: 0 },
                    { time: "2:45 PM", metric: 0 },
                    { time: "3:00 PM", metric: 0 },
                    { time: "3:15 PM", metric: 1 },
                    { time: "3:30 PM", metric: 0 },
                    { time: "3:45 PM", metric: 0 },
                    { time: "4:00 PM", metric: 0 },
                  ],
                  xAxisKey: "time",
                  lineKeys: [
                    {
                      key: "metric",
                      name: "errors",
                      color: "var(--chart-line-4)",
                    },
                  ],
                }}
                syncId="fnMetrics"
              />
              <SingleGraph
                title="Execution Time"
                data={{
                  data: [
                    { time: "12:00 PM", p50: 42, p90: 67, p95: 89 },
                    { time: "12:15 PM", p50: 43, p90: 68, p95: 90 },
                    { time: "12:30 PM", p50: 44, p90: 69, p95: 91 },
                    { time: "12:45 PM", p50: 44, p90: 70, p95: 92 },
                    { time: "1:00 PM", p50: 45, p90: 71, p95: 93 },
                    { time: "1:15 PM", p50: 45, p90: 72, p95: 94 },
                    { time: "1:30 PM", p50: 44, p90: 71, p95: 93 },
                    { time: "1:45 PM", p50: 45, p90: 72, p95: 94 },
                    { time: "2:00 PM", p50: 45, p90: 72, p95: 94 },
                    { time: "2:15 PM", p50: 44, p90: 71, p95: 93 },
                    { time: "2:30 PM", p50: 43, p90: 70, p95: 92 },
                    { time: "2:45 PM", p50: 42, p90: 69, p95: 91 },
                    { time: "3:00 PM", p50: 41, p90: 68, p95: 90 },
                    { time: "3:15 PM", p50: 42, p90: 67, p95: 89 },
                    { time: "3:30 PM", p50: 41, p90: 66, p95: 88 },
                    { time: "3:45 PM", p50: 42, p90: 67, p95: 89 },
                    { time: "4:00 PM", p50: 41, p90: 65, p95: 88 },
                  ],
                  xAxisKey: "time",
                  lineKeys: [
                    {
                      key: "p50",
                      name: "ms p50",
                      color: "var(--chart-line-1)",
                    },
                    {
                      key: "p90",
                      name: "ms p90",
                      color: "var(--chart-line-2)",
                    },
                    {
                      key: "p95",
                      name: "ms p95",
                      color: "var(--chart-line-3)",
                    },
                  ],
                }}
                syncId="fnMetrics"
              />
              <SingleGraph
                title="Cache Hit Rate"
                data={{
                  data: [
                    { time: "12:00 PM", metric: 95 },
                    { time: "12:15 PM", metric: 95 },
                    { time: "12:30 PM", metric: 96 },
                    { time: "12:45 PM", metric: 96 },
                    { time: "1:00 PM", metric: 97 },
                    { time: "1:15 PM", metric: 97 },
                    { time: "1:30 PM", metric: 97 },
                    { time: "1:45 PM", metric: 97 },
                    { time: "2:00 PM", metric: 97 },
                    { time: "2:15 PM", metric: 97 },
                    { time: "2:30 PM", metric: 96 },
                    { time: "2:45 PM", metric: 96 },
                    { time: "3:00 PM", metric: 96 },
                    { time: "3:15 PM", metric: 96 },
                    { time: "3:30 PM", metric: 96 },
                    { time: "3:45 PM", metric: 96 },
                    { time: "4:00 PM", metric: 96 },
                  ],
                  xAxisKey: "time",
                  lineKeys: [
                    {
                      key: "metric",
                      name: "%",
                      color: "var(--chart-line-1)",
                    },
                  ],
                }}
                syncId="fnMetrics"
              />
            </div>
          </div>
        </div>
      </div>

      {/* Main content */}
      <div className="absolute inset-0 flex items-center justify-center">
        <Sheet className="m-6 size-fit bg-background-secondary/90 p-2 backdrop-blur-[2px]">
          <EmptySection
            Icon={CodeIcon}
            color="yellow"
            header="No functions in this deployment, yet."
            body="Deploy some functions to get started."
            learnMoreButton={{
              href: "https://docs.convex.dev/quickstarts",
              children: "Follow a quickstart guide",
            }}
            sheet={false}
          />
        </Sheet>
      </div>
    </div>
  );
}
