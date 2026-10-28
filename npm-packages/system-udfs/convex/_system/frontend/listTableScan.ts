import { GenericDocument } from "convex/server";
import { queryGeneric } from "../secretSystemTables";
import { v } from "convex/values";

export default queryGeneric("ViewData")({
  args: {
    table: v.string(),
    limit: v.number(),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async ({ db }, { table, limit }): Promise<GenericDocument[]> => {
    if (!table) {
      return [];
    }

    const documents: GenericDocument[] = [];
    const query = (db.query(table).order("desc") as any).limit(limit);
    try {
      for await (const doc of query) {
        documents.push(doc);
      }
    } catch (error: any) {
      // Catch error indicating we are reading too much and truncate the result.
      if (!error.message.includes("too large")) {
        throw error;
      }
    }

    return documents;
  },
});
