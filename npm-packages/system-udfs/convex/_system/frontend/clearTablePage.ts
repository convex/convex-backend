import { Cursor } from "convex/server";
import { mutationGeneric, writeAuditLog } from "../server";
import { v } from "convex/values";

export const MAX_CLEAR_ROWS = 4000;

export default mutationGeneric("WriteData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
    tableName: v.string(),
    cursor: v.union(v.string(), v.null()),
  },
  handler: async (
    ctx,
    args,
  ): Promise<{
    deleted: number;
    hasMore: boolean;
    continueCursor: Cursor;
  }> => {
    const { db } = ctx;
    const { tableName, cursor } = args;
    // Delete from oldest to newest to avoid repeatedly invalidating the query
    // run by the data page
    const {
      page: documents,
      continueCursor,
      isDone,
    } = await db
      .query(tableName)
      .withIndex("by_creation_time")
      .order("asc")
      .paginate({
        numItems: MAX_CLEAR_ROWS,
        cursor,
        // We can read up to 8MiB, but we're currently double counting the docs
        // when they're read in this query and also when they're deleted.
        // And we allow `maximumBytesRead` to be exceeded by a single document,
        // and a single document maxes out at 1MiB.
        // To ensure (maximumBytesRead + 1MiB) * 2 < 8MiB, we need
        // this limit conservative to avoid hitting limits and crashing.
        maximumBytesRead: 3000000,
      });

    await Promise.all(documents.map((doc) => db.delete(doc._id)));

    if (documents.length > 0) {
      await writeAuditLog("delete_documents", {
        table: tableName,
        document_ids: documents.map((doc) => doc._id),
      });
    }

    return {
      deleted: documents.length,
      continueCursor,
      hasMore: !isDone,
    };
  },
});
