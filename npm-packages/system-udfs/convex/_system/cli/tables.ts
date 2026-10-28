import { paginationOptsValidator } from "convex/server";
import { queryPrivateSystem } from "../secretSystemTables";
import { performOp } from "udf-syscall-ffi";

export default queryPrivateSystem("ViewData")({
  args: {
    paginationOpts: paginationOptsValidator,
  },
  handler: async () => {
    const tables: Record<number, string> = performOp("getTableMapping");
    // We don't need to paginate but keep the PaginationResult return type for backwards
    // compatibility.
    return {
      page: Object.values(tables)
        .map((name) => ({ name }))
        .filter(({ name }) => !name.startsWith("_")),
      isDone: true,
      continueCursor: "end",
    };
  },
});
