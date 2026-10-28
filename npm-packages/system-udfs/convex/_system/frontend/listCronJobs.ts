import { CronJob, CronJobWithRuns } from "./common";
import { queryPrivateSystem } from "../secretSystemTables";
import { v } from "convex/values";
export default queryPrivateSystem("ViewData")({
  args: { componentId: v.optional(v.union(v.string(), v.null())) },
  handler: async ({ db }): Promise<CronJobWithRuns[]> => {
    const jobs: CronJob[] = await db.query("_cron_jobs").collect();
    const jobsWithRuns: CronJobWithRuns[] = [];

    for (const job of jobs) {
      const lastRun = await db
        .query("_cron_job_logs")
        .withIndex("by_name_and_ts", (q) => q.eq("name", job.name))
        .order("desc")
        .first();
      const nextRun = await db
        .query("_cron_next_run")
        .withIndex("by_cron_job_id", (q) => q.eq("cronJobId", job._id))
        .first();
      if (nextRun === null) {
        throw new Error("No next run found for cron job");
      }
      jobsWithRuns.push({
        ...job,
        lastRun: lastRun,
        nextRun: nextRun,
      });
    }

    return jobsWithRuns;
  },
});
