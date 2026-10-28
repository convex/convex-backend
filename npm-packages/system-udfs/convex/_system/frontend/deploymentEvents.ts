import { Doc } from "../../_generated/dataModel";
import { queryPrivateSystem } from "../secretSystemTables";
import { v } from "convex/values";

type PushConfigEvent = Doc<"_deployment_audit_log"> & {
  action: "push_config" | "push_config_with_components";
};

export const lastPushEvent = queryPrivateSystem("ViewData")({
  args: {
    componentId: v.optional(v.union(v.string(), v.null())),
  },
  handler: async function ({ db }): Promise<PushConfigEvent | null> {
    const lastPushEvent = await db
      .query("_deployment_audit_log")
      // eslint-disable-next-line @convex-dev/no-filter-in-query -- FIXME: _deployment_audit_log has no by_action index
      .filter((q) =>
        q.or(
          q.eq(q.field("action"), "push_config"),
          q.eq(q.field("action"), "push_config_with_components"),
        ),
      )
      .order("desc")
      .first();

    // Making the type system happy. This should never happen
    // because the query should always return a push event.
    if (
      lastPushEvent &&
      lastPushEvent.action !== "push_config" &&
      lastPushEvent.action !== "push_config_with_components"
    ) {
      return null;
    }

    return lastPushEvent;
  },
});
