import { convexToJson } from "convex/values";
import { ConvexReactClient, useSubscription } from "convex/react";
import isEqual from "lodash/isEqual";
import { useMemo, useState } from "react";
import { StopIcon } from "@radix-ui/react-icons";
import { FunctionResult } from "convex/browser";
import {
  DefaultFunctionArgs,
  FunctionReference,
  getFunctionName,
  makeFunctionReference,
} from "convex/server";
import { Tooltip } from "@ui/Tooltip";
import * as FunctionTypes from "@common/lib/functions/types";
import { Result } from "@common/features/functionRunner/components/Result";

export function QueryResult({
  module,
  parameters,
  reactClient,
  paused,
  onCopiedQueryResult,
}: {
  reactClient: ConvexReactClient;
  module: FunctionTypes.ModuleFunction;
  parameters: DefaultFunctionArgs;
  paused: boolean;
  onCopiedQueryResult?: () => void;
}) {
  if (module.udfType !== "Query") {
    throw new Error("Invalid udf type");
  }

  const functionReference = makeFunctionReference<"query">(module.displayName);
  const { componentPath } = module;
  const result = useQueryWithLogs(
    reactClient,
    functionReference,
    parameters,
    componentPath ?? undefined,
  );

  // Show stale result while the next is loading
  const [cachedResult, setCachedResult] = useState(result);
  if (result !== undefined && !isEqual(result, cachedResult)) {
    setCachedResult(result);
  }

  return (
    <div className="flex size-full flex-col gap-4">
      <Result
        result={cachedResult}
        loading={result === undefined}
        queryStatus={
          paused ? (
            <Tooltip
              tip="The arguments are invalid. Fix the argument errors to continue."
              side="left"
            >
              <StopIcon className="text-content-errorSecondary" />
            </Tooltip>
          ) : result && !result.success ? (
            <Tooltip
              tip="This function call encountered an error. Try changing the arguments or authentication to try again."
              side="left"
            >
              <StopIcon className="text-content-errorSecondary" />
            </Tooltip>
          ) : (
            <Tooltip tip="This query is subscribed to updates." side="left">
              <div className="flex items-center gap-1 text-sm text-green-700 select-none motion-safe:animate-blink dark:text-green-200">
                <div className="size-2.5 rounded-full bg-green-700 dark:bg-green-200" />{" "}
              </div>
            </Tooltip>
          )
        }
        requestFilter={null}
        startCursor={0}
        onCopiedQueryResult={onCopiedQueryResult}
      />
    </div>
  );
}

function useQueryWithLogs<
  Func extends FunctionReference<"query", "public", any, any>,
>(
  convex: ConvexReactClient,
  func: Func,
  args: Func["_args"],
  componentPath?: string,
): FunctionResult | undefined {
  const watch = useMemo(
    () => convex.watchQuery(func, args, { componentPath }),
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [getFunctionName(func), convex, JSON.stringify(convexToJson(args))],
  );

  const subscription = useMemo(
    () => ({
      getCurrentValue: () => {
        const logLines = watch.localQueryLogs() || [];
        try {
          const value = watch.localQueryResult();
          if (value === undefined) {
            return undefined;
          }
          return {
            success: true,
            value,
            logLines,
          } as const;
        } catch (error: any) {
          return {
            success: false,
            errorMessage: error.toString(),
            logLines,
          } as const;
        }
      },
      subscribe: (callback: () => void) => watch.onUpdate(callback),
    }),
    [watch],
  );

  const queryResult = useSubscription(subscription);
  return queryResult;
}
