import { actionGeneric } from "convex/server";
import { v } from "convex/values";
import type { ListModels } from "./modelList";

export function defineListModelsAction(listModels: ListModels) {
  return actionGeneric({
    args: {},
    returns: v.object({
      object: v.string(),
      count: v.number(),
      firstModel: v.union(v.string(), v.null()),
    }),
    handler: async () => {
      return await listModels();
    },
  });
}
