import { Export } from "./common";
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

export const canExportFileStorage = queryGeneric("ViewBackups")({
  args: {},
  handler: async (ctx) => {
    // Allow files to be exported if the `_storage` table number matches
    // the default table number, which is in this ID.
    const sampleStorageId = "kg27rxfv99gzp01wmph0gvt92d6hnvy6";
    return ctx.db.system.normalizeId("_storage", sampleStorageId) !== null;
  },
});
