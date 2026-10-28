import { ReactNode, useContext, useMemo } from "react";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import {
  CronSpec,
  Module,
  CronJobWithRuns,
  CronJobLog,
} from "system-udfs/convex/_system/frontend/common";
import { useInMemoryDocumentCache } from "@common/features/schedules/lib/useInMemoryDocumentCache";
import { useListModules } from "@common/lib/functions/useListModules";
import { createContextHook } from "@common/lib/createContextHook";
import { useNents } from "@common/lib/useNents";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

type CronJobsContextType = {
  cronsModule: Module | undefined;
  cronJobs: CronJobWithRuns[] | undefined;
  loading: boolean;
  cronJobRuns: CronJobLog[] | undefined;
};

export const [CronJobsContext, useCronJobs] =
  createContextHook<CronJobsContextType>({
    name: "CronJobs",
  });

export function CronJobsProviderWithCronHistory({
  children,
}: {
  children: ReactNode;
}) {
  const { captureMessage } = useContext(DeploymentInfoContext);
  // Get functions
  const modules = useListModules();
  // Get cron jobs
  const cronJobs: CronJobWithRuns[] | undefined = useQuery(
    udfs.listCronJobs.default,
    { componentId: useNents().selectedNent?.id || null },
  );

  const currentCronJobRuns = useQuery(udfs.listCronJobRuns.default, {
    componentId: useNents().selectedNent?.id || null,
  });
  // Backends only persists the last 5 runs of a cron job.
  // Cache old runs to keep them from disappearing while a developer reads logs.
  const cronJobRuns = useInMemoryDocumentCache(currentCronJobRuns);

  // This might be a new typed (source mapped cron jobs) in the future.
  const [orderedCronJobs, cronsModule]: [
    CronJobWithRuns[] | undefined,
    Module | undefined,
  ] = useMemo(() => {
    if (!cronJobs || !modules || !cronJobRuns) return [undefined, undefined];

    let cronsModuleInner: Module | undefined;
    // Load cron specs from `_modules (as well as cron jobs from `_cron_jobs`)
    // for source order and (TODO(tom)) source mapping.
    let cronSpecs: Map<string, CronSpec> | undefined;
    for (const [name, mod] of modules.entries()) {
      if (mod.cronSpecs) {
        if (cronSpecs) {
          void Promise.reject(new Error("Crons found on multiple modules"));
        }
        if (name !== "crons.js") {
          void Promise.reject(
            new Error(`Crons found on unexpected module: ${name}`),
          );
          continue;
        }
        cronSpecs = new Map(mod.cronSpecs);
        cronsModuleInner = mod;
      }
    }
    if (!cronSpecs) return [undefined, cronsModuleInner];

    const cronJobsMap = new Map<string, CronJobWithRuns>();
    for (const cronJob of cronJobs) {
      cronJobsMap.set(cronJob.name, cronJob);
    }
    return [
      [...cronSpecs.keys()]
        .map((identifier) => {
          const cronJob = cronJobsMap.get(identifier)!;
          if (!cronJob) {
            captureMessage(
              `No CronJob found for CronSpec ${identifier}`,
              "error",
            );
          }
          return cronJob;
        })
        .filter((x) => x), // remove empty
      cronsModuleInner,
    ];
  }, [cronJobs, modules, cronJobRuns, captureMessage]);

  return (
    <CronJobsContext.Provider
      value={{
        cronsModule,
        cronJobs: orderedCronJobs,
        loading: !(cronJobs && cronJobRuns),
        cronJobRuns,
      }}
    >
      {children}
    </CronJobsContext.Provider>
  );
}

export function CronJobsProvider({ children }: { children: ReactNode }) {
  return (
    <CronJobsProviderWithCronHistory>
      {children}
    </CronJobsProviderWithCronHistory>
  );
}
