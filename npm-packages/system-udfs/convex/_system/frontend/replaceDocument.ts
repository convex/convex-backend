import { ConvexError, GenericId } from "convex/values";
import { v } from "convex/values";
import { decodeId } from "id-encoding";
import { performOp } from "udf-syscall-ffi";
import { mutationGeneric, writeAuditLog } from "../server";

export default mutationGeneric("WriteData")({
  args: {
    id: v.string(),
    document: v.any(),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (
    { db },
    args,
  ): Promise<{ success: false; error: string } | { success: true }> => {
    const id = args.id as GenericId<string>;
    const document = args.document;
    const doc = await db.get(id);
    if (doc === null) {
      return {
        success: false,
        error: "Document does not exist.",
      };
    }

    try {
      await db.replace(id, document);
    } catch (e: any) {
      // Rewrapping this error because it could be a schema validation error.
      throw new ConvexError(e.message);
    }
    const tableMapping: Record<number, string> = performOp("getTableMapping");
    const table = tableMapping[decodeId(args.id).tableNumber];
    await writeAuditLog("update_documents", {
      table,
      document_ids: [args.id],
    });
    return { success: true };
  },
});
