import { paginationOptsValidator, PaginationResult } from "convex/server";
import { Export } from "./common";
import { maximumBytesRead, maximumRowsRead } from "../paginationLimits";
import { queryGeneric, queryPrivateSystem } from "../secretSystemTables";
export default queryPrivateSystem("ViewBackups")({
  args: {},
  handler: async function ({ db }): Promise<Export | null> {
    return await db
      .query("_exports")
      .withIndex("by_requestor", (q) => q.eq("requestor", "snapshotExport"))
      .order("desc")
      .first();
  },
});

export const latestCloudExport = queryPrivateSystem("ViewBackups")({
  args: {},
  handler: async function ({ db }): Promise<Export | null> {
    return await db
      .query("_exports")
      .withIndex("by_requestor", (q) => q.eq("requestor", "cloudBackup"))
      .order("desc")
      .first();
  },
});

type CloudBackup = Pick<Export, "_id" | "_creationTime" | "state"> & {
  format: Pick<Export["format"], "include_storage">;
  expiration_ts?: bigint;
  complete_ts?: bigint;
  progress_message?: string | null;
};

export const listCloudBackups = queryPrivateSystem("ViewBackups")({
  args: { paginationOpts: paginationOptsValidator },
  handler: async function (
    { db },
    { paginationOpts },
  ): Promise<PaginationResult<CloudBackup>> {
    const { page, isDone, continueCursor, splitCursor, pageStatus } = await db
      .query("_exports")
      .withIndex("by_requestor", (q) => q.eq("requestor", "cloudBackup"))
      .order("desc")
      .paginate({ ...paginationOpts, maximumBytesRead, maximumRowsRead });

    return {
      isDone,
      continueCursor,
      splitCursor,
      pageStatus,
      page: page.map((backup) => ({
        _id: backup._id,
        _creationTime: backup._creationTime,
        state: backup.state,
        format: { include_storage: backup.format.include_storage },
        expiration_ts:
          "expiration_ts" in backup ? backup.expiration_ts : undefined,
        complete_ts:
          backup.state === "completed" ? backup.complete_ts : undefined,
        progress_message:
          backup.state === "in_progress" ? backup.progress_message : undefined,
      })),
    };
  },
});

export const canExportFileStorage = queryGeneric("ViewBackups")({
  args: {},
  handler: async (ctx) => {
    // Allow files to be exported if the `_storage` table number matches
    // the default table number, which is in this ID.
    const sampleStorageId = "kg27rxfv99gzp01wmph0gvt92d6hnvy6";
    return ctx.db.system.normalizeId("_storage", sampleStorageId) !== null;
  },
});
