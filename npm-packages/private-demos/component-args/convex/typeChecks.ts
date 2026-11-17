import type { FunctionHandle } from "convex/server";
import type { Doc } from "../jobs/_generated/dataModel";

export function toFunctionHandle(
  handle: Doc<"jobs">["onComplete"],
): FunctionHandle<"mutation", { jobId: string; result: string }, null> {
  // @ts-expect-error `FunctionHandle_future` isn't assignable to `FunctionHandle`.
  return handle;
}
