import { Doc } from "../../_generated/dataModel";
import { queryPrivateSystem } from "../secretSystemTables";
import { v } from "convex/values";
import { clampForAuditLogRetention } from "./paginatedDeploymentEvents";

/**
 * Get the deployment events on or after the provided timestamp from least recent
 * to most recent
 */
export default queryPrivateSystem("ViewAuditLog")({
  args: { fromTimestamp: v.number() },
  handler: async function (
    { db },
    { fromTimestamp },
  ): Promise<Doc<"_deployment_audit_log">[]> {
    fromTimestamp = await clampForAuditLogRetention(db, fromTimestamp);
    return await db
      .query("_deployment_audit_log")
      .withIndex("by_creation_time", (q) =>
        q.gte("_creationTime", fromTimestamp),
      )
      .order("asc")
      .collect();
  },
});
