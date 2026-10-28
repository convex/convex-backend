import { GenericId, v } from "convex/values";
import { mutationGeneric, writeAuditLog } from "../server";

const MAX_DOCUMENT_DELETIONS = 4096;

export default mutationGeneric("WriteData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
    toDelete: v.array(v.object({ id: v.string(), tableName: v.string() })),
  },
  handler: async (
    { db },
    { toDelete },
  ): Promise<{ success: false; error: string } | { success: true }> => {
    if (toDelete.length > MAX_DOCUMENT_DELETIONS) {
      return {
        success: false,
        error: `You can't delete more than ${MAX_DOCUMENT_DELETIONS}. Try selecting fewer documents instead..`,
      };
    }
    for (const d of toDelete) {
      await db.delete(d.id as GenericId<string>);
    }
    // Group by table and emit one audit log per table
    const byTable = new Map<string, string[]>();
    for (const d of toDelete) {
      const ids = byTable.get(d.tableName) ?? [];
      ids.push(d.id);
      byTable.set(d.tableName, ids);
    }
    for (const [table, document_ids] of byTable) {
      await writeAuditLog("delete_documents", {
        table,
        document_ids,
      });
    }
    return { success: true };
  },
});
