import {
  KeyboardIcon,
  Crosshair2Icon,
  InfoCircledIcon,
  ChevronDownIcon,
  ChevronUpIcon,
  DotFilledIcon,
} from "@radix-ui/react-icons";
import { Fragment, useCallback, useRef, useState } from "react";
import {
  TabList as HeadlessTabList,
  TabPanels as HeadlessTabPanels,
  TabPanel as HeadlessTabPanel,
  TabGroup as HeadlessTabGroup,
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import { Panel, PanelGroup, PanelResizeHandle } from "react-resizable-panels";
import { MAX_LOGS, UdfLog, UdfLogOutput } from "@common/lib/useLogs";
import { ClosePanelButton } from "@ui/ClosePanelButton";
import { Button } from "@ui/Button";
import { LiveTimestampDistance } from "@common/elements/TimestampDistance";
import { LogLevel } from "@common/elements/LogLevel";
import { LogStatusLine } from "@common/features/logs/components/LogStatusLine";
import { LogOutput, messagesToString } from "@common/elements/LogOutput";
import { CopyButton } from "@common/elements/CopyButton";
import { DeploymentEventContent } from "@common/elements/DeploymentEventContent";
import { Tab } from "@ui/Tab";
import { Link } from "@ui/Link";
import { useHotkeys } from "react-hotkeys-hook";
import { KeyboardShortcut } from "@ui/KeyboardShortcut";
import { Callout } from "@ui/Callout";
import { ITEM_SIZE } from "@common/features/logs/components/LogListItem";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { cn } from "@ui/cn";
import { FunctionCallTree } from "./FunctionCallTree";
import { LogMetadata } from "./LogMetadata";
import { InterleavedLog, getTimestamp, getLogKey } from "../lib/interleaveLogs";

export function LogDrilldown({
  requestId,
  onClose,
  selectedLog: selectedLogProp,
  onFilterByRequestId,
  onSelectLog,
  onHitBoundary,
  shownInterleavedLogs,
  allUdfLogs,
  logListContainerRef,
}: {
  requestId?: string;
  shownInterleavedLogs: InterleavedLog[];
  allUdfLogs: UdfLog[];
  onClose: () => void;
  selectedLog: InterleavedLog;
  onFilterByRequestId?: (requestId: string) => void;
  onSelectLog: (log: InterleavedLog) => void;
  onHitBoundary: (boundary: "top" | "bottom" | null) => void;
  logListContainerRef?: React.RefObject<HTMLDivElement | null>;
}) {
  const [selectedTabIndex, setSelectedTabIndex] = useState(0);
  const tabGroupRef = useRef<HTMLDivElement>(null);
  const rightPanelRef = useRef<HTMLDivElement>(null);

  const selectedLog = selectedLogProp;

  useNavigateLogs(
    selectedLog,
    shownInterleavedLogs,
    onSelectLog,
    onClose,
    onHitBoundary,
    rightPanelRef,
    logListContainerRef,
  );

  if (!selectedLog) {
    return null;
  }

  if (selectedLog.kind === "ClearedLogs") {
    return <div className="size-full border-l bg-background-primary/70" />;
  }

  return (
    <div className="flex h-full max-h-full flex-col overflow-hidden border-l bg-background-primary/70">
      {/* Header */}
      <div className="border-b bg-background-secondary px-2 py-4">
        <div className="flex flex-wrap items-center justify-between gap-4">
          <h4 className="flex flex-wrap items-center gap-2">
            <div className="flex flex-wrap items-center gap-2">
              {selectedLog.kind === "ExecutionLog" ? (
                <>
                  <span className="font-mono text-sm">
                    {selectedLog.executionLog.localizedTimestamp}
                    <span className="text-content-secondary">
                      .
                      {new Date(getTimestamp(selectedLog))
                        .toISOString()
                        .split(".")[1]
                        .slice(0, -1)}
                    </span>
                  </span>
                  <span className="text-xs font-normal text-nowrap text-content-secondary">
                    (
                    <LiveTimestampDistance
                      date={new Date(getTimestamp(selectedLog))}
                      className="inline"
                    />
                    )
                  </span>
                  <div className="font-mono text-xs">
                    {selectedLog.executionLog.kind === "log" &&
                      selectedLog.executionLog.output.level && (
                        <LogLevel
                          level={selectedLog.executionLog.output.level}
                        />
                      )}
                    {selectedLog.executionLog.kind === "outcome" && (
                      <LogStatusLine
                        outcome={selectedLog.executionLog.outcome}
                      />
                    )}
                  </div>
                </>
              ) : (
                <>
                  <span className="font-mono text-sm">
                    {new Date(getTimestamp(selectedLog)).toLocaleString()}
                  </span>
                  <span className="text-xs font-normal text-nowrap text-content-secondary">
                    (
                    <LiveTimestampDistance
                      date={new Date(getTimestamp(selectedLog))}
                      className="inline"
                    />
                    )
                  </span>
                </>
              )}
            </div>
          </h4>
          <div className="flex items-center gap-1">
            {selectedLog.kind === "ExecutionLog" && requestId && (
              <Button
                icon={<Crosshair2Icon />}
                variant="neutral"
                size="xs"
                inline
                tip="View log line in context"
                onClick={() => {
                  onFilterByRequestId?.(requestId);
                }}
              />
            )}
            <ClosePanelButton onClose={onClose} />
          </div>
        </div>
      </div>

      <div
        ref={rightPanelRef}
        tabIndex={-1}
        className="scrollbar grow animate-fadeInFromLoading overflow-y-auto"
      >
        {/* Callout - outside of resizable panels */}
        {selectedLog.kind === "ExecutionLog" &&
          requestId &&
          allUdfLogs.length === 0 && (
            <Callout
              className="mx-2 mb-2 flex items-center gap-2 p-1.5 text-xs"
              variant="upsell"
            >
              <InfoCircledIcon className="min-w-4" />
              <div>
                Heads up! The logs page only keeps track of the latest{" "}
                {MAX_LOGS.toLocaleString()} logs. This panel is missing
                information for this log because it is older than the maximum
                number of logs. If you need log persistence, try out{" "}
                <Link
                  href="https://docs.convex.dev/production/integrations/log-streams/"
                  target="_blank"
                >
                  Log Streams
                </Link>
                .
              </div>
            </Callout>
          )}

        <LogContentLayout
          selectedLog={selectedLog}
          allUdfLogs={allUdfLogs}
          selectedTabIndex={selectedTabIndex}
          setSelectedTabIndex={setSelectedTabIndex}
          tabGroupRef={tabGroupRef}
        />
      </div>

      <KeyboardShortcutsSection selectedLog={selectedLog} />
    </div>
  );
}

const shortcutItemClass = "grid grid-cols-[1fr_9.5rem] gap-x-2 min-w-0";
const shortcutKeysClass = "flex items-center justify-end gap-1";
const shortcutLabelClass = "truncate min-w-0";

function KeyboardShortcutsSection({
  selectedLog,
}: {
  selectedLog: InterleavedLog;
}) {
  const [isOpen, setIsOpen] = useGlobalLocalStorage(
    "logDrilldown.keyboardShortcuts.open",
    false,
  );

  return (
    <section className="border-t bg-background-tertiary px-4 py-2">
      <Disclosure defaultOpen={isOpen}>
        {({ open }) => (
          <>
            <div className="flex items-center justify-between">
              <DisclosureButton
                className="flex items-center gap-1 text-xs"
                onClick={() => setIsOpen(!isOpen)}
              >
                <KeyboardIcon className="relative -left-0.5 text-content-secondary" />
                <h6 className="font-semibold text-content-secondary">
                  Keyboard Shortcuts
                </h6>
                {open ? (
                  <ChevronUpIcon className="size-3" />
                ) : (
                  <ChevronDownIcon className="size-3" />
                )}
              </DisclosureButton>
            </div>

            <DisclosurePanel className="mt-2 scrollbar animate-fadeInFromLoading overflow-x-auto">
              <div className="grid grid-cols-[16.5rem_14rem] gap-x-4 gap-y-1 text-xs text-content-secondary">
                <div className={shortcutItemClass}>
                  <div className={shortcutKeysClass}>
                    <KeyboardShortcut value={["Down"]} />
                    <span>/</span>
                    <KeyboardShortcut value={["Up"]} />
                  </div>
                  <span className={shortcutLabelClass}>Navigate</span>
                </div>

                <div className={shortcutItemClass}>
                  <div className={shortcutKeysClass}>
                    <KeyboardShortcut value={["CtrlOrCmd"]} />
                    <span>+</span>
                    <KeyboardShortcut value={["A"]} />
                  </div>
                  <span className={shortcutLabelClass}>Jump to top</span>
                </div>

                {selectedLog.kind === "ExecutionLog" && (
                  <>
                    <div className={shortcutItemClass}>
                      <div className={shortcutKeysClass}>
                        <KeyboardShortcut value={["Shift"]} />
                        <span>+</span>
                        <KeyboardShortcut value={["Down"]} />
                        <span>/</span>
                        <KeyboardShortcut value={["Up"]} />
                      </div>
                      <span className={shortcutLabelClass}>
                        Navigate request
                      </span>
                    </div>

                    <div className={shortcutItemClass}>
                      <div className={shortcutKeysClass}>
                        <KeyboardShortcut value={["CtrlOrCmd"]} />
                        <span>+</span>
                        <KeyboardShortcut value={["E"]} />
                      </div>
                      <span className={shortcutLabelClass}>Jump to bottom</span>
                    </div>

                    <div className={shortcutItemClass}>
                      <div className={shortcutKeysClass}>
                        <KeyboardShortcut value={["CtrlOrCmd"]} />
                        <span>+</span>
                        <KeyboardShortcut value={["Down"]} />
                        <span>/</span>
                        <KeyboardShortcut value={["Up"]} />
                      </div>
                      <span className={shortcutLabelClass}>
                        Navigate execution
                      </span>
                    </div>

                    <div className={shortcutItemClass}>
                      <div className={shortcutKeysClass}>
                        <KeyboardShortcut value={["CtrlOrCmd"]} />
                        <KeyboardShortcut value={["Shift"]} />
                        <span>+</span>
                        <KeyboardShortcut value={["A"]} />
                      </div>
                      <span className={shortcutLabelClass}>
                        Jump to top of request
                      </span>
                    </div>
                  </>
                )}

                <div className={shortcutItemClass}>
                  <div className={shortcutKeysClass}>
                    <KeyboardShortcut value={["CtrlOrCmd"]} />
                    <span>+</span>
                    <KeyboardShortcut value={["PageUp"]} />
                    <span>/</span>
                    <KeyboardShortcut value={["PageDown"]} />
                  </div>
                  <span className={shortcutLabelClass}>Navigate page</span>
                </div>

                {selectedLog.kind === "ExecutionLog" ? (
                  <div className={shortcutItemClass}>
                    <div className={shortcutKeysClass}>
                      <KeyboardShortcut value={["CtrlOrCmd"]} />
                      <KeyboardShortcut value={["Shift"]} />
                      <span>+</span>
                      <KeyboardShortcut value={["E"]} />
                    </div>
                    <span className={shortcutLabelClass}>
                      Jump to bottom of request
                    </span>
                  </div>
                ) : (
                  <div className={shortcutItemClass}>
                    <div className={shortcutKeysClass}>
                      <KeyboardShortcut value={["CtrlOrCmd"]} />
                      <span>+</span>
                      <KeyboardShortcut value={["E"]} />
                    </div>
                    <span className={shortcutLabelClass}>Jump to bottom</span>
                  </div>
                )}

                <div className={shortcutItemClass}>
                  <div className={shortcutKeysClass}>
                    <KeyboardShortcut value={["Shift"]} />
                    <span>+</span>
                    <KeyboardShortcut value={["Right"]} />
                  </div>
                  <span className={shortcutLabelClass}>Focus this panel</span>
                </div>

                <div className={shortcutItemClass}>
                  <div className={shortcutKeysClass}>
                    <KeyboardShortcut value={["Esc"]} />
                  </div>
                  <span className={shortcutLabelClass}>Close this panel</span>
                </div>
              </div>
            </DisclosurePanel>
          </>
        )}
      </Disclosure>
    </section>
  );
}

export function useNavigateLogs(
  selectedLog: InterleavedLog | null,
  logs: InterleavedLog[],
  onSelectLog: (log: InterleavedLog) => void,
  onClose: () => void,
  onHitBoundary: (boundary: "top" | "bottom" | null) => void,
  rightPanelRef: React.RefObject<HTMLDivElement | null>,
  logListContainerRef?: React.RefObject<HTMLDivElement | null>,
) {
  // Calculate the number of items that fit in one page based on container height
  const calculatePageSize = useCallback(() => {
    if (!logListContainerRef?.current) {
      return 10; // Default fallback
    }
    const containerHeight = logListContainerRef.current.clientHeight;
    return Math.floor(containerHeight / ITEM_SIZE);
  }, [logListContainerRef]);
  // Get logs for the current execution (both log entries and outcomes)
  const executionLogs =
    selectedLog && selectedLog.kind === "ExecutionLog"
      ? logs.filter(
          (log) =>
            log.kind === "ExecutionLog" &&
            log.executionLog.executionId ===
              selectedLog.executionLog.executionId,
        )
      : undefined;

  // Get logs for the current request (both log entries and outcomes)
  const requestLogs =
    selectedLog && selectedLog.kind === "ExecutionLog"
      ? logs.filter(
          (log) =>
            log.kind === "ExecutionLog" &&
            log.executionLog.requestId === selectedLog.executionLog.requestId,
        )
      : undefined;

  // Navigate to prev/next log
  const navigateLog = useCallback(
    (direction: "prev" | "next", scope: "execution" | "request" | "all") => {
      if (!selectedLog || !onSelectLog) return;

      let targetLogs: InterleavedLog[] | undefined;
      if (scope === "execution") {
        targetLogs = executionLogs;
      } else if (scope === "request") {
        targetLogs = requestLogs;
      } else {
        targetLogs = logs;
      }

      if (!targetLogs) return;

      const selectedLogKey = getLogKey(selectedLog);
      const currentIndex = targetLogs.findIndex(
        (log) => getLogKey(log) === selectedLogKey,
      );

      if (currentIndex === -1) {
        console.warn("Current log not found in target logs", {
          selectedLog,
          targetLogsCount: targetLogs.length,
          scope,
        });
        return;
      }

      const nextIndex =
        direction === "prev" ? currentIndex - 1 : currentIndex + 1;
      if (nextIndex >= 0 && nextIndex < targetLogs.length) {
        onSelectLog(targetLogs[nextIndex]);
        onHitBoundary(null);
      } else {
        // Hit the boundary - trigger animation
        onHitBoundary(direction === "next" ? "bottom" : "top");
      }
    },
    [selectedLog, executionLogs, requestLogs, logs, onSelectLog, onHitBoundary],
  );

  // Keyboard shortcuts
  useHotkeys("down", () => navigateLog("next", "all"), {
    preventDefault: true,
  });
  useHotkeys("up", () => navigateLog("prev", "all"), {
    preventDefault: true,
  });
  useHotkeys("shift+down", () => navigateLog("next", "request"), {
    preventDefault: true,
  });
  useHotkeys("shift+up", () => navigateLog("prev", "request"), {
    preventDefault: true,
  });
  useHotkeys(
    ["ctrl+down", "meta+down"],
    () => navigateLog("next", "execution"),
    {
      preventDefault: true,
    },
  );
  useHotkeys(["meta+up", "ctrl+up"], () => navigateLog("prev", "execution"), {
    preventDefault: true,
  });
  // Handle arrow key navigation between log list and right panel
  useHotkeys(
    "shift+right",
    () => {
      // Focus the right panel content area
      if (rightPanelRef.current) {
        rightPanelRef.current.focus();
      }
    },
    {
      preventDefault: true,
    },
  );

  useHotkeys(
    "esc",
    () => {
      onClose();
    },
    {
      preventDefault: true,
    },
  );

  // Navigate to top/bottom of list
  useHotkeys(
    ["ctrl+a", "meta+a"],
    () => {
      if (logs.length > 0) {
        onSelectLog(logs[0]);
        onHitBoundary(null);
      }
    },
    {
      preventDefault: true,
    },
  );
  useHotkeys(
    ["ctrl+e", "meta+e"],
    () => {
      if (logs.length > 0) {
        onSelectLog(logs[logs.length - 1]);
        onHitBoundary(null);
      }
    },
    {
      preventDefault: true,
    },
  );

  // Navigate to top/bottom within request
  useHotkeys(
    ["ctrl+shift+a", "meta+shift+a"],
    () => {
      if (requestLogs && requestLogs.length > 0) {
        onSelectLog(requestLogs[0]);
        onHitBoundary(null);
      }
    },
    {
      preventDefault: true,
    },
  );
  useHotkeys(
    ["ctrl+shift+e", "meta+shift+e"],
    () => {
      if (requestLogs && requestLogs.length > 0) {
        onSelectLog(requestLogs[requestLogs.length - 1]);
        onHitBoundary(null);
      }
    },
    {
      preventDefault: true,
    },
  );

  // Navigate by page (based on container height)
  useHotkeys(
    ["ctrl+pageup", "meta+pageup"],
    () => {
      if (!selectedLog) return;
      const pageSize = calculatePageSize();
      const selectedLogKey = getLogKey(selectedLog);
      const currentIndex = logs.findIndex(
        (log) => getLogKey(log) === selectedLogKey,
      );
      if (currentIndex === -1) return;

      const newIndex = Math.max(0, currentIndex - pageSize);
      onSelectLog(logs[newIndex]);
      if (newIndex === 0) {
        onHitBoundary("top");
      } else {
        onHitBoundary(null);
      }
    },
    {
      preventDefault: true,
    },
    [selectedLog, logs, onSelectLog, onHitBoundary, calculatePageSize],
  );
  useHotkeys(
    ["ctrl+pagedown", "meta+pagedown"],
    () => {
      if (!selectedLog) return;
      const pageSize = calculatePageSize();
      const selectedLogKey = getLogKey(selectedLog);
      const currentIndex = logs.findIndex(
        (log) => getLogKey(log) === selectedLogKey,
      );
      if (currentIndex === -1) return;

      const newIndex = Math.min(logs.length - 1, currentIndex + pageSize);
      onSelectLog(logs[newIndex]);
      if (newIndex === logs.length - 1) {
        onHitBoundary("bottom");
      } else {
        onHitBoundary(null);
      }
    },
    {
      preventDefault: true,
    },
    [selectedLog, logs, onSelectLog, onHitBoundary, calculatePageSize],
  );
}

function LogMessageCard({
  output,
  title = "Log Message",
  isError = false,
  maxHeight,
}: {
  output: UdfLogOutput;
  title?: string;
  isError?: boolean;
  maxHeight?: string;
}) {
  const copyText = `${messagesToString(output as any)}${output.isTruncated ? " (truncated due to length)" : ""}`;

  return (
    <div
      className={cn(
        "animate-fadeInFromLoading rounded-md border",
        "mx-2 flex h-full flex-col overflow-hidden",
        isError
          ? "bg-background-error text-content-error"
          : "bg-background-secondary",
      )}
    >
      <div className="mb-1 flex items-center justify-between gap-1 px-2 pt-2">
        <p className="text-xs font-semibold">{title}</p>
        <CopyButton
          text={copyText}
          className={isError ? "text-content-error" : ""}
          inline
        />
      </div>
      <div
        className={cn(
          "scrollbar overflow-y-auto px-2 pb-2 font-mono text-xs",
          maxHeight,
        )}
      >
        <LogOutput output={output} wrap />
      </div>
    </div>
  );
}

function LogContentLayout({
  selectedLog,
  allUdfLogs,
  selectedTabIndex,
  setSelectedTabIndex,
  tabGroupRef,
}: {
  selectedLog: InterleavedLog;
  allUdfLogs: UdfLog[];
  selectedTabIndex: number;
  setSelectedTabIndex: (index: number) => void;
  tabGroupRef: React.RefObject<HTMLDivElement | null>;
}) {
  const hasLogOutput =
    selectedLog.kind === "DeploymentEvent" ||
    (selectedLog.kind === "ExecutionLog" &&
      selectedLog.executionLog.kind === "log") ||
    (selectedLog.kind === "ExecutionLog" &&
      selectedLog.executionLog.kind === "outcome" &&
      selectedLog.executionLog.error);

  const showTabs = selectedLog.kind === "ExecutionLog";

  const renderTabs = (log: UdfLog, className?: string) => (
    <div className={cn("relative flex flex-col", className)}>
      <HeadlessTabGroup
        as={Fragment}
        selectedIndex={selectedTabIndex}
        onChange={setSelectedTabIndex}
      >
        <div className="sticky top-0 z-10 px-2" ref={tabGroupRef}>
          <HeadlessTabList className="flex gap-1 rounded-t-md border bg-background-secondary px-1">
            <Tab>Execution</Tab>
            <Tab>Request</Tab>
            <Tab>Functions Called</Tab>
          </HeadlessTabList>
        </div>

        <div className="mx-2 h-full min-h-0 overflow-hidden rounded-sm rounded-t-none border border-t-0 bg-background-secondary">
          <div className="scrollbar flex h-full flex-col gap-2 overflow-y-auto">
            <HeadlessTabPanels>
              <HeadlessTabPanel>
                <LogMetadata
                  requestId={log.requestId}
                  logs={allUdfLogs}
                  executionId={log.executionId}
                />
              </HeadlessTabPanel>
              <HeadlessTabPanel>
                <LogMetadata
                  requestId={log.requestId}
                  logs={allUdfLogs}
                  executionId={undefined}
                />
              </HeadlessTabPanel>
              <HeadlessTabPanel>
                <FunctionCallTree logs={allUdfLogs} currentLog={log} />
              </HeadlessTabPanel>
            </HeadlessTabPanels>
          </div>
        </div>
      </HeadlessTabGroup>
    </div>
  );

  // Render log output content
  const renderLogOutput = () => (
    <>
      {/* Deployment Event Content */}
      {selectedLog.kind === "DeploymentEvent" && (
        <div className="m-2 mt-0 animate-fadeInFromLoading rounded-md border bg-background-secondary">
          <div className="p-2">
            <p className="mb-1 text-xs font-semibold">Deployment Event</p>
            <DeploymentEventContent
              event={selectedLog.deploymentEvent}
              inPanel
            />
          </div>
        </div>
      )}

      {/* Selected Log Output - show when a log is selected and it's not a completion */}
      {selectedLog.kind === "ExecutionLog" &&
        selectedLog.executionLog.kind === "log" && (
          <LogMessageCard
            output={selectedLog.executionLog.output}
            isError={selectedLog.executionLog.output.level === "ERROR"}
          />
        )}

      {/* Error message for outcome logs with errors */}
      {selectedLog.kind === "ExecutionLog" &&
        selectedLog.executionLog.kind === "outcome" &&
        selectedLog.executionLog.error && (
          <LogMessageCard
            output={{
              isTruncated: false,
              messages: [selectedLog.executionLog.error],
              level: "FAILURE",
            }}
            title="Error"
            isError
          />
        )}
    </>
  );

  // If both sections exist, use resizable panels
  if (hasLogOutput && showTabs) {
    return (
      <PanelGroup direction="vertical" autoSaveId="log-drilldown-content">
        {/* First Panel: Deployment Event or Log Output/Error */}
        <Panel
          id="log-output-panel"
          order={0}
          defaultSize={50}
          minSize={10}
          className="max-h-fit"
        >
          <div className="h-full overflow-hidden pt-2">{renderLogOutput()}</div>
        </Panel>

        {/* Resize Handle */}
        <PanelResizeHandle
          className="group relative"
          hitAreaMargins={{ coarse: 8, fine: 8 }}
        >
          <div className="flex h-2 items-center justify-center bg-background-primary/70 transition-all group-hover:bg-util-accent/10">
            <DotFilledIcon className="text-content-tertiary/50" />
          </div>
        </PanelResizeHandle>

        {/* Second Panel: Tabs */}
        <Panel
          id="log-tabs-panel"
          order={1}
          defaultSize={50}
          minSize={20}
          className="pb-2"
        >
          {renderTabs(selectedLog.executionLog, "h-full")}
        </Panel>
      </PanelGroup>
    );
  }

  // Otherwise, render without PanelGroup
  return (
    <div className="my-2">
      {hasLogOutput && renderLogOutput()}
      {showTabs && renderTabs(selectedLog.executionLog, "grow")}
    </div>
  );
}
