import classNames from "classnames";
import { useQuery } from "convex/react";
import { useRouter } from "next/router";
import { useEffect, useCallback, useContext } from "react";
import { usePrevious } from "react-use";
import udfs from "@common/udfs";
import { useHotkeys } from "react-hotkeys-hook";
import { Button } from "@ui/Button";
import { FunctionIcon } from "@common/elements/icons";
import { toast } from "@common/lib/utils";
import {
  useHideGlobalRunner,
  useIsGlobalRunnerShown,
  useShowGlobalRunner,
  useGlobalRunnerSelectedItem,
} from "@common/features/functionRunner/lib/functionRunner";
import { GlobalFunctionTester } from "@common/features/functionRunner/components/FunctionTester";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";

export function FunctionRunnerWrapper({
  isVertical,
  setIsVertical,
  isExpanded,
  setIsExpanded,
  onRanCustomQuery,
  onCopiedQueryResult,
}: {
  isVertical: boolean;
  setIsVertical: (isVertical: boolean) => void;
  isExpanded: boolean;
  setIsExpanded: (isExpanded: boolean) => void;
  onRanCustomQuery?: () => void;
  onCopiedQueryResult?: () => void;
}) {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const deploymentState = useQuery(
    udfs.deploymentState.deploymentState,
    canViewData ? {} : "skip",
  );
  const router = useRouter();
  const { query } = router;
  const previousQuery = usePrevious(query);

  const isGlobalRunnerShown = useIsGlobalRunnerShown();
  const showGlobalRunner = useShowGlobalRunner();
  const hideGlobalRunner = useHideGlobalRunner();
  const [globalRunnerSelectedItem, setGlobalRunnerSelectedItem] =
    useGlobalRunnerSelectedItem();

  useEffect(() => {
    if (
      previousQuery?.team !== query.team ||
      previousQuery?.project !== query.project ||
      previousQuery?.deploymentName !== query.deploymentName
    ) {
      if (isGlobalRunnerShown) {
        hideGlobalRunner("redirect");
      }
      if (globalRunnerSelectedItem !== null) {
        setGlobalRunnerSelectedItem(null);
      }
    }
  }, [
    previousQuery?.team,
    previousQuery?.project,
    previousQuery?.deploymentName,
    query.team,
    query.project,
    query.deploymentName,
    query.deployment,
    hideGlobalRunner,
    isGlobalRunnerShown,
    globalRunnerSelectedItem,
    setGlobalRunnerSelectedItem,
  ]);

  useHotkeys("ctrl+`", () => {
    if (deploymentState?.state === "paused") {
      toast(
        "error",
        "You cannot run functions while the deployment is paused.",
      );
      return;
    }
    if (isGlobalRunnerShown) {
      hideGlobalRunner("keyboard");
    } else {
      showGlobalRunner(null, "keyboard");
    }
  });

  const { useLogDeploymentEvent } = useContext(DeploymentInfoContext);
  const log = useLogDeploymentEvent();

  const setIsGlobalRunnerVerticalAndLog = useCallback(
    (vertical: boolean) => {
      setIsVertical(vertical);
      log("change function runner orientation", {
        orientation: vertical ? "vertical" : "horizontal",
      });
    },
    [log, setIsVertical],
  );

  return (
    <>
      <GlobalFunctionTester
        isVertical={!!isVertical}
        setIsVertical={setIsGlobalRunnerVerticalAndLog}
        isExpanded={isExpanded}
        setIsExpanded={setIsExpanded}
        onRanCustomQuery={onRanCustomQuery}
        onCopiedQueryResult={onCopiedQueryResult}
      />
      {!isGlobalRunnerShown && deploymentState?.state !== "paused" && (
        <Button
          variant="unstyled"
          className={classNames(
            "fixed bottom-16 right-0",
            "group flex items-center gap-2 rounded-l-2xl border-border-selected border-y border-l bg-background-secondary/85 p-2",
            "backdrop-blur-[2px]",
            !isGlobalRunnerShown && "w-12 hover:w-40",
          )}
          onClick={() => {
            showGlobalRunner(null, "click");
          }}
        >
          <div className="size-8">
            <FunctionIcon className="size-8" />
          </div>

          <div className="ml-1 flex-col items-center gap-1 whitespace-nowrap transition-all select-none">
            Run functions
            <div className="flex w-full items-center gap-0.5 text-xs">
              Shortcut:
              <kbd className="font-sans font-semibold">⌃</kbd> +
              <kbd className="font-sans font-semibold">`</kbd>
            </div>
          </div>
        </Button>
      )}
    </>
  );
}
