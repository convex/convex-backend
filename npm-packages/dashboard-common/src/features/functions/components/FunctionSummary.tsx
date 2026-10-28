import {
  Link2Icon,
  PlayIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { useQuery } from "convex/react";
import { useContext, useEffect, useRef, useState } from "react";
import { Portal } from "@headlessui/react";
import { lt } from "semver";
import udfs from "@common/udfs";
import { UdfType } from "system-udfs/convex/_system/frontend/common";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { CopiedPopper } from "@common/elements/CopiedPopper";
import { copyTextToClipboard } from "@common/lib/utils";
import { useHttpActionRoute } from "@common/features/functions/lib/useHttpActionRoute";
import { FunctionRunnerDisabledWhilePaused } from "@common/features/functions/components/FunctionRunnerDisabledWhilePaused";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import { useShowGlobalRunner } from "@common/features/functionRunner/lib/functionRunner";
import { ModuleFunction } from "@common/lib/functions/types";
import { Loading } from "@ui/Loading";
import { AuthorizeEditsConfirmationDialog } from "@common/elements/AuthorizeEditsConfirmationDialog";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { useEditsAuthorization } from "@common/features/data/lib/useEditsAuthorization";

export function FunctionSummary({
  currentOpenFunction,
}: {
  currentOpenFunction: ModuleFunction;
}) {
  const { areEditsAuthorized, authorizeEdits } = useEditsAuthorization();
  const [showAuthorizeEditsModal, setShowAuthorizeEditsModal] = useState(false);

  const npmPackageVersion = useQuery(udfs.getVersion.default);
  const versionTooOld = !!npmPackageVersion && lt(npmPackageVersion, "0.13.0");

  const { useIsDeploymentPaused } = useContext(DeploymentInfoContext);
  const { useIsOperationAllowed } = useContext(PermissionsContext);

  const udfType = currentOpenFunction.udfType;
  const canRunInternalQueries = useIsOperationAllowed("RunInternalQueries");
  const canRunInternalMutations = useIsOperationAllowed("RunInternalMutations");
  const canRunInternalActions = useIsOperationAllowed("RunInternalActions");
  const canViewData = useIsOperationAllowed("ViewData");
  const canWriteData = useIsOperationAllowed("WriteData");

  const isInternal = currentOpenFunction.visibility?.kind === "internal";
  const isInComponent = !!currentOpenFunction.componentPath;

  const httpActionRoute = useHttpActionRoute(currentOpenFunction);

  const copyUrlButtonRef = useRef<HTMLElement | null>(null);
  const [copiedPopperElement, setCopiedPopperElement] =
    useState<HTMLDivElement | null>(null);
  const [didJustCopyUrl, setDidJustCopyUrl] = useState(false);
  useEffect(() => {
    if (didJustCopyUrl) {
      const timeout = setTimeout(() => setDidJustCopyUrl(false), 800);
      return () => clearTimeout(timeout);
    }
  }, [didJustCopyUrl]);

  const canRunFunction = (() => {
    if (isInternal) {
      return udfType === "Query"
        ? canRunInternalQueries
        : udfType === "Mutation"
          ? canRunInternalMutations
          : canRunInternalActions;
    }
    if (isInComponent) {
      return udfType === "Query" ? canViewData : canWriteData;
    }
    return true;
  })();

  const showGlobalRunner = useShowGlobalRunner();
  const showFunctionRunner = () => {
    showGlobalRunner(currentOpenFunction, "click");
  };

  const isPaused = useIsDeploymentPaused();
  if (isPaused === undefined) {
    return <Loading />;
  }
  return (
    <div className="flex h-full flex-col overflow-hidden">
      <div className="flex flex-wrap items-end justify-between gap-2 pb-2">
        {showAuthorizeEditsModal && (
          <AuthorizeEditsConfirmationDialog
            onClose={() => {
              setShowAuthorizeEditsModal(false);
            }}
            onConfirm={async () => {
              authorizeEdits?.();
              setShowAuthorizeEditsModal(false);
              showFunctionRunner();
            }}
          />
        )}
        <div className="flex flex-wrap items-center gap-2">
          <div className="flex flex-wrap items-center gap-x-2">
            <h3 className="font-mono">{currentOpenFunction.name}</h3>
            <div
              className={`rounded-sm p-1 text-xs font-semibold ${getFunctionTypeStyles(currentOpenFunction.udfType).text} ${getFunctionTypeStyles(currentOpenFunction.udfType).background}`}
            >
              {currentOpenFunction.visibility.kind === "internal" &&
                "Internal "}
              {functionTypeLabel(currentOpenFunction.udfType)}
            </div>
          </div>
          {currentOpenFunction.displayName !== currentOpenFunction.name && (
            <CopyTextButton
              className="font-mono"
              text={currentOpenFunction.displayName}
            />
          )}
          {httpActionRoute?.status === "unmounted" && (
            <Tooltip
              asChild
              side="bottom"
              maxWidthClassName="max-w-xs"
              tip={
                <div className="text-left text-pretty">
                  <p className="mb-0.5">
                    <code>{currentOpenFunction.componentPath}</code> is
                    installed without an <code>httpPrefix</code>, so its HTTP
                    routes aren’t served.
                  </p>
                  <p>
                    Set an <code>httpPrefix</code> in{" "}
                    <code>convex/convex.config.ts</code> to mount the routes.
                  </p>
                </div>
              }
            >
              <span className="flex items-center gap-1 rounded-full bg-background-warning px-2 py-1 text-xs text-content-warning">
                Not mounted
                <QuestionMarkCircledIcon className="size-3.5 min-w-3.5" />
              </span>
            </Tooltip>
          )}
          {httpActionRoute?.status === "mounted" && (
            <>
              <Button
                ref={copyUrlButtonRef}
                variant="neutral"
                size="xs"
                inline
                icon={<Link2Icon />}
                aria-label="Copy URL"
                tip="Copy URL"
                tipSide="bottom"
                onClick={() => {
                  void copyTextToClipboard(httpActionRoute.url).then(() =>
                    setDidJustCopyUrl(true),
                  );
                }}
              />
              <Portal>
                <CopiedPopper
                  referenceElement={copyUrlButtonRef.current}
                  copiedPopperElement={copiedPopperElement}
                  setCopiedPopperElement={setCopiedPopperElement}
                  show={didJustCopyUrl}
                  message="Copied URL"
                  placement="bottom"
                />
              </Portal>
            </>
          )}
        </div>
        {
          // Supported UDF types for in-dashboard testing
          ["Query", "Mutation", "Action"].some(
            (udfType) => udfType === currentOpenFunction.udfType,
          ) && (
            <Button
              tip={
                !canRunFunction ? (
                  "You do not have permission to run this function in this deployment."
                ) : isPaused ? (
                  <FunctionRunnerDisabledWhilePaused />
                ) : (
                  versionTooOld && (
                    <div>
                      The function runner is only available on deployments using
                      Convex version 0.13.0 or greater.
                    </div>
                  )
                )
              }
              disabled={isPaused || versionTooOld || !canRunFunction}
              onClick={() =>
                currentOpenFunction.udfType === "Query" || areEditsAuthorized
                  ? showFunctionRunner()
                  : setShowAuthorizeEditsModal(true)
              }
              icon={<PlayIcon />}
              size="xs"
              variant="primary"
            >
              Run Function
            </Button>
          )
        }
      </div>
    </div>
  );
}

export const functionTypeLabel = (udfType: UdfType) => {
  let text = "";
  switch (udfType) {
    case "Query":
      text = "Query";
      break;
    case "Mutation":
      text = "Mutation";
      break;
    case "Action":
      text = "Action";
      break;
    case "HttpAction":
      text = "HTTP";
      break;
    default:
      udfType satisfies never;
      text = "Function";
  }
  return text;
};

export const getFunctionTypeStyles = (udfType: UdfType) => {
  switch (udfType) {
    case "Query":
      return {
        text: "text-yellow-700 dark:text-yellow-200",
        background: "bg-yellow-100/50 dark:bg-yellow-900/50",
      };
    case "Mutation":
      return {
        text: "text-blue-700 dark:text-blue-200",
        background: "bg-blue-100/50 dark:bg-blue-900/50",
      };
    case "Action":
      return {
        text: "text-purple-700 dark:text-purple-200",
        background: "bg-purple-100/50 dark:bg-purple-900/50",
      };
    case "HttpAction":
      return {
        text: "text-green-700 dark:text-green-200",
        background: "bg-green-100/50 dark:bg-green-900/50",
      };
    default:
      return {
        text: "text-content-primary",
        background: "bg-background-tertiary",
      };
  }
};
