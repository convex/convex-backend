"use node";
import { log } from "convex/server";
import { action } from "./_generated/server";

export const loggingNodeAction = action({
  args: {},
  handler: async () => {
    await log.audit({ source: "loggingNodeAction" });
  },
});
