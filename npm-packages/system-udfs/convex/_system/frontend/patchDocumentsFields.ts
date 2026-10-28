import { Value, v, GenericId, ConvexError } from "convex/values";
import { mutationGeneric, writeAuditLog } from "../server";
import { UNDEFINED_PLACEHOLDER } from "./lib/values";

export default mutationGeneric("WriteData")({
  args: {
    table: v.string(),
    fields: v.any(),
    ids: v.optional(v.array(v.string())),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async (
    ctx,
    args,
  ): Promise<{ success: false; error: string } | { success: true }> => {
    const fields = args.fields as Record<
      string,
      Value | typeof UNDEFINED_PLACEHOLDER
    >;
    const ids = args.ids as GenericId<string>[] | undefined;
    const documents =
      ids !== undefined
        ? await Promise.all(ids.map((id) => ctx.db.get(id)))
        : await ctx.db.query(args.table).collect();
    try {
      const patchFields: Record<string, Value | undefined> = {};
      for (const key in fields) {
        const value = fields[key];
        patchFields[key] = value === UNDEFINED_PLACEHOLDER ? undefined : value;
      }

      await Promise.all(
        documents.map((document) => ctx.db.patch(document._id, patchFields)),
      );
    } catch (e: any) {
      // Rewrapping this error because it could be a schema validation error.
      throw new ConvexError(e.message);
    }
    await writeAuditLog("update_documents", {
      table: args.table,
      document_ids: documents.map((doc) => doc._id),
    });
    return { success: true };
  },
});
