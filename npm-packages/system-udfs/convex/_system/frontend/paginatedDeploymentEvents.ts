import { paginationOptsValidator } from "convex/server";
import { queryPrivateSystem } from "../secretSystemTables";
import { v } from "convex/values";
import { maximumBytesRead, maximumRowsRead } from "../paginationLimits";
import { DatabaseReader } from "../../_generated/server";

/**
 * Paginated query for the deployment events from most recent to least recent
 */
export default queryPrivateSystem("ViewAuditLog")({
  args: {
    paginationOpts: paginationOptsValidator,
    filters: v.object({
      minDate: v.number(),
      maxDate: v.optional(v.number()),
      authorMemberIds: v.optional(v.array(v.int64())),
      actions: v.optional(v.array(v.string())),
    }),
  },
  handler: async function ({ db }, { paginationOpts, filters }) {
    filters.minDate = await clampForAuditLogRetention(db, filters.minDate);

    const paginatedResults = await db
      .query("_deployment_audit_log")
      .withIndex("by_creation_time", (q) => {
        const partial = q.gte("_creationTime", filters.minDate);

        return filters.maxDate
          ? partial.lte("_creationTime", filters.maxDate)
          : partial;
      })
      .order("desc")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- we allow filtering by multiple member IDs/actions
      .filter((q) => {
        // FIXME: Note that here, we could use an index for the case where we filter for a single member ID and/or a single action

        const queryFilters = [];
        if (filters.authorMemberIds !== undefined) {
          queryFilters.push(
            q.or(
              ...filters.authorMemberIds.map((memberId) =>
                q.eq(memberId, q.field("member_id")),
              ),
            ),
          );
        }
        if (filters.actions !== undefined) {
          queryFilters.push(
            q.or(
              ...filters.actions.map((action) =>
                q.eq(action, q.field("action")),
              ),
            ),
          );
        }
        return q.and(...queryFilters);
      })
      .paginate({
        ...paginationOpts,
        maximumBytesRead,
        maximumRowsRead,
      });

    return paginatedResults;
  },
});

export async function clampForAuditLogRetention(
  db: DatabaseReader,
  minDate: number,
) {
  const backendInfo = await db.query("_backend_info").first();
  const auditLogRetentionDays = Number(backendInfo?.auditLogRetentionDays || 0);
  // no limit if auditLogRetentionDays is -1
  if (auditLogRetentionDays === -1) {
    return minDate;
  }
  const minAllowable =
    Date.now() - (auditLogRetentionDays + 1) * 24 * 60 * 60 * 1000;
  if (minDate < minAllowable) {
    return minAllowable;
  }
  return minDate;
}
