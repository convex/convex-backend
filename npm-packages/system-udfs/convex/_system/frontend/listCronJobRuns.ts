import { v } from "convex/values";
import { queryPrivateSystem } from "../secretSystemTables";
import { CronJobLog } from "./common";

export default queryPrivateSystem("ViewData")({
  args: { componentId: v.optional(v.union(v.string(), v.null())) },
  handler: async ({ db }): Promise<CronJobLog[]> => {
    const logs: CronJobLog[] = await db.query("_cron_job_logs").collect();
    return logs;
  },
});
