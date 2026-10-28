import { v } from "convex/values";
import { mutationGeneric, writeAuditLog } from "../server";

export default mutationGeneric("WriteData")({
  args: {
    table: v.string(),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async ({ db }, { table }) => {
    // We don't have an "official" way to create a table without inserting documents,
    // but inserting a document and deleting it in the same transaction is one way to do this.
    const id = await db.insert(table, {});
    await db.delete(id);
    await writeAuditLog("create_table", {
      table,
    });
  },
});
