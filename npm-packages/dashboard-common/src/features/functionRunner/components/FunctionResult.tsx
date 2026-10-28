import { LockOpen2Icon, PlayIcon } from "@radix-ui/react-icons";
import classNames from "classnames";
import type { FunctionResult as FunctionResultType } from "convex/browser";
import { useContext, useEffect, useState } from "react";
import { Value } from "convex/values";
import { Button } from "@ui/Button";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { PermissionDeniedTip } from "@common/elements/NoPermissionMessage";
import { deploymentIdentityLabel } from "@common/lib/deploymentTypeColorClasses";
import { toast } from "@common/lib/utils";
import { RequestFilter } from "@common/lib/appMetrics";
import { ComponentId } from "@common/lib/useNents";
import { Visibility } from "system-udfs/convex/_system/frontend/common";
import { Result } from "@common/features/functionRunner/components/Result";
import {
  useRunHistory,
  RunHistoryItem,
  useImpersonatedUser,
  useIsImpersonating,
} from "@common/features/functionRunner/components/RunHistory";
import { useEditsAuthorization } from "@common/features/data/lib/useEditsAuthorization";
import { RoleStatementAction } from "@convex-dev/platform/managementApi";

// This is a hook because we want to return composable components that can be arranged
// vertically or horizontally.
export function useFunctionResult({
  udfType,
  visibility,
  isInComponent,
  onSubmit,
  disabled,
  functionIdentifier,
  componentId,
  args,
  runHistoryItem,
  onCopiedQueryResult,
}: {
  udfType?: "Mutation" | "Action" | "Query" | "HttpAction";
  visibility?: Visibility | null;
  isInComponent?: boolean;
  onSubmit(): {
    requestFilter: RequestFilter | null;
    runFunctionPromise: Promise<FunctionResultType> | null;
  };
  disabled: boolean;
  functionIdentifier?: string;
  componentId: ComponentId;
  args: Record<string, Value>;
  runHistoryItem?: RunHistoryItem;
  onCopiedQueryResult?: () => void;
}) {
  const { appendRunHistory } = useRunHistory(
    functionIdentifier || "",
    componentId,
  );
  const [isInFlight, setIsInFlight] = useState(false);
  const [lastRequestTiming, setLastRequestTiming] = useState<{
    startedAt: number;
    endedAt: number;
  }>();

  const [, setIsImpersonating] = useIsImpersonating();
  const [, setImpersonatedUser] = useImpersonatedUser();

  const [result, setResult] = useState<FunctionResultType>();
  const [requestFilter, setRequestFilter] = useState<RequestFilter | null>(
    null,
  );
  const [startCursor, setStartCursor] = useState<number>(0);

  const isInvalidUdfType =
    !udfType || !["Mutation", "Action"].includes(udfType);
  useEffect(() => {
    if (!isInvalidUdfType) {
      setResult(undefined);
      setLastRequestTiming(undefined);
      setIsInFlight(false);
    }
  }, [isInvalidUdfType]);

  useEffect(() => {
    setResult(undefined);
    setLastRequestTiming(undefined);
    setIsInFlight(false);
  }, [functionIdentifier]);

  useEffect(() => {
    if (runHistoryItem) {
      setResult(undefined);
      setLastRequestTiming(undefined);
      setStartCursor(0);
      if (runHistoryItem.type === "arguments") {
        setIsImpersonating(!!runHistoryItem.user);
        if (runHistoryItem.user) {
          setImpersonatedUser(runHistoryItem.user);
        }
      }
    }
  }, [runHistoryItem, setImpersonatedUser, setIsImpersonating]);

  const { useCurrentDeployment, useLogDeploymentEvent } = useContext(
    DeploymentInfoContext,
  );
  const { useIsOperationAllowed } = useContext(PermissionsContext);

  const deployment = useCurrentDeployment();
  const dtype = deployment?.deploymentType;
  const isProd = dtype === "prod";
  const deploymentIdentity = deployment
    ? deploymentIdentityLabel(deployment)
    : null;

  const { areEditsAuthorized, authorizeEdits } = useEditsAuthorization();
  const log = useLogDeploymentEvent();
  const canRunInternalQueries = useIsOperationAllowed("RunInternalQueries");
  const canRunInternalMutations = useIsOperationAllowed("RunInternalMutations");
  const canRunInternalActions = useIsOperationAllowed("RunInternalActions");
  const canViewData = useIsOperationAllowed("ViewData");
  const canWriteData = useIsOperationAllowed("WriteData");

  const isInternal = visibility?.kind === "internal";

  const canRunFunction = (() => {
    if (isInternal) {
      // Internal functions require explicit RunInternal* permissions
      return udfType === "Query"
        ? canRunInternalQueries
        : udfType === "Mutation"
          ? canRunInternalMutations
          : canRunInternalActions;
    }
    if (isInComponent) {
      // Public functions in a component require ViewData (queries) or WriteData (mutations/actions)
      return udfType === "Query" ? canViewData : canWriteData;
    }
    // Public functions at the top level are always allowed
    return true;
  })();

  // The specific role action that would unblock this Run button — used
  // by the disabled-state tooltip so custom-role members see exactly
  // which grant is missing.
  const missingRunAction = (() => {
    if (isInternal) {
      if (udfType === "Query") return "deployment:functions:runInternalQueries";
      if (udfType === "Mutation")
        return "deployment:functions:runInternalMutations";
      return "deployment:functions:runInternalActions";
    }
    if (isInComponent) {
      return udfType === "Query"
        ? "deployment:data:view"
        : "deployment:data:write";
    }
    return null;
  })();

  if (isInvalidUdfType) {
    return { button: null, result: null };
  }
  const runFunction = async () => {
    const startedAt = Date.now();
    setResult(undefined);
    setIsInFlight(true);
    const { requestFilter: requestFilterResult, runFunctionPromise } =
      onSubmit();
    setRequestFilter(requestFilterResult);
    setStartCursor(startedAt);
    let functionResult: FunctionResultType | undefined;
    try {
      functionResult = await runFunctionPromise!;
      log("run function", {
        function: {
          identifier: functionIdentifier,
          udfType,
        },
        success: functionResult.success,
        isProd,
      });
    } catch (e: any) {
      functionResult = {
        success: false,
        errorMessage: e.message,
        logLines: [],
      };
    } finally {
      // Wait a moment before re-enable the button to
      // avoid the user accidently re-running the function.
      setTimeout(() => {
        setIsInFlight(false);
      }, 100);
      const endedAt = Date.now();
      setLastRequestTiming({
        startedAt,
        endedAt,
      });
      setResult(functionResult);
      appendRunHistory({
        type: "arguments",
        startedAt,
        endedAt,
        arguments: args,
      });
    }
  };

  return {
    button: (
      <div className={classNames("flex items-center gap-2 mx-4")}>
        <Button
          tip={
            disabled ? (
              "Fix the errors above to continue."
            ) : !canRunFunction ? (
              missingRunAction ? (
                <PermissionDeniedTip
                  message="You do not have permission to run this function in this deployment."
                  action={missingRunAction as RoleStatementAction}
                />
              ) : (
                "You do not have permission to run this function in this deployment."
              )
            ) : !areEditsAuthorized ? (
              `You are about to run a ${udfType.toLowerCase()} in ${
                deploymentIdentity === null
                  ? "this deployment"
                  : `the ${deploymentIdentity} deployment`
              }. Unlock edits to continue.`
            ) : undefined
          }
          size="sm"
          className="w-full max-w-3xl items-center justify-center"
          disabled={disabled || !areEditsAuthorized || !canRunFunction}
          loading={isInFlight}
          onClick={runFunction}
          icon={<PlayIcon />}
        >
          Run {udfType.toLowerCase()}
        </Button>
        {canRunFunction && !areEditsAuthorized && (
          <Button
            tip={`Enables changes to ${
              deploymentIdentity === null
                ? "this deployment"
                : `the ${deploymentIdentity} deployment`
            } for the remainder of this dashboard session`}
            size="sm"
            onClick={() => {
              authorizeEdits?.();
              toast(
                "success",
                "Edits enabled for the remainder of this dashboard session",
              );
            }}
            icon={<LockOpen2Icon />}
          >
            Unlock Edits
          </Button>
        )}
      </div>
    ),
    result: (
      <Result
        result={result}
        loading={isInFlight}
        lastRequestTiming={lastRequestTiming}
        requestFilter={requestFilter}
        startCursor={startCursor}
        onCopiedQueryResult={onCopiedQueryResult}
      />
    ),
    runFunction,
  };
}
