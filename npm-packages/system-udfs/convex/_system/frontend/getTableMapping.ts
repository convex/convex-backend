import { v } from "convex/values";
import { performOp } from "udf-syscall-ffi";
import { queryPrivateSystem } from "../secretSystemTables";

/**
 * Returns an object mapping the table numbers to table names
 * (e.g. {"1": "users"})
 */
export default queryPrivateSystem("ViewData")({
  args: { componentId: v.optional(v.union(v.string(), v.null())) },
  handler: async () => {
    const tableMapping: Record<number, string> =
      await performOp("getTableMapping");
    return Object.fromEntries(
      Object.entries(tableMapping)
        .filter(
          ([, name]) =>
            !name.startsWith("_") ||
            ["_scheduled_jobs", "_file_storage"].includes(name),
          //
        )
        .slice(0, 1000),
    );
  },
});
