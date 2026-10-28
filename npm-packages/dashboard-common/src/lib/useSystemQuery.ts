import { type UseQueryResult, useQuery_experimental } from "convex/react";
import {
  type FunctionArgs,
  type FunctionReference,
  type FunctionReturnType,
} from "convex/server";
import { useContext, useEffect } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

type SystemQuery = FunctionReference<"query">;

const PERMISSION_DENIED =
  /You do not have permission to perform this operation(?:\s*\(([^)]+)\))?/;

export function permissionDenial(error: Error): { action?: string } | null {
  const match = error.message.match(PERMISSION_DENIED);
  return match === null ? null : { action: match[1]?.trim() };
}

export function useSystemQuery<Query extends SystemQuery>(
  query: Query,
  args: FunctionArgs<Query> | "skip",
): UseQueryResult<FunctionReturnType<Query>> {
  const { captureException } = useContext(DeploymentInfoContext);
  const state = useQuery_experimental({ query, args });

  const error = state.status === "error" ? state.error : undefined;
  // A restricted role hitting a UDF it can't run is an expected outcome, not a
  // fault worth paging on.
  const unexpectedError =
    error && permissionDenial(error) === null ? error : undefined;
  useEffect(() => {
    if (unexpectedError) {
      captureException(unexpectedError);
    }
    // The error object is rebuilt per update; key the report off its message.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [captureException, unexpectedError?.message]);

  return state;
}
