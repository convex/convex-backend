import { GenericId, v } from "convex/values";
import { queryGeneric } from "../secretSystemTables";
import { SystemTableNames } from "convex/server";

export default queryGeneric("ViewData")({
  args: {
    id: v.string(),
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async function ({ db }, args) {
    const id = args.id as GenericId<string>;
    try {
      return await db.get(id);
    } catch {
      return await db.system.get(id as GenericId<SystemTableNames>);
    }
  },
});
