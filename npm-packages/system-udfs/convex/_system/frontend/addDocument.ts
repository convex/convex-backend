import { GenericDocument } from "convex/server";
import { mutationGeneric, writeAuditLog } from "../server";

import { ConvexError, v } from "convex/values";

const MAX_IMPORT_COUNT = 4096; // TRANSACTION_MAX_NUM_USER_WRITES / 2

export default mutationGeneric("WriteData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
    table: v.string(),
    documents: v.array(v.any()),
  },
  handler: async (
    { db },
    args,
  ): Promise<{ success: false; error: string } | { success: true }> => {
    const { table } = args;
    const documents = args.documents as GenericDocument[];

    if (documents.length > MAX_IMPORT_COUNT) {
      return {
        success: false,
        error: `Can’t import more than ${MAX_IMPORT_COUNT} documents at once.`,
      };
    }

    const insertedIds = [];
    try {
      for (const document of documents) {
        const id = await db.insert(table, document);
        insertedIds.push(id);
      }
    } catch (e: any) {
      // Revert the documents previously inserted since we return
      // a result and thus make the transaction commit.
      for (const id of insertedIds) {
        await db.delete(id);
      }

      // Rewrapping this error because it could be a schema validation error.
      throw new ConvexError(e.message);
    }
    await writeAuditLog("add_documents", {
      table,
      document_ids: insertedIds,
    });
    return { success: true };
  },
});
