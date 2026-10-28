import { requestFromConvexJson } from "./23_request.js";
import { convexJsonFromResponse } from "./23_response.js";
import { throwUncatchableDeveloperError } from "./helpers.js";
import { getBlob, storeBlob } from "./storage.js";

export function setupJsSyscall(global: any) {
  global.Convex.jsSyscall = (op: string, args: Record<string, any>) => {
    switch (op) {
      case "requestFromConvexJson":
        return requestFromConvexJson(args as any);
      case "convexJsonFromResponse":
        return convexJsonFromResponse(args as any);
      case "storage/storeBlob":
        return storeBlob(args as any);
      case "storage/getBlob":
        return getBlob(args as any);
      default:
        return throwUncatchableDeveloperError(`Unknown JS syscall: ${op}`);
    }
  };
}
