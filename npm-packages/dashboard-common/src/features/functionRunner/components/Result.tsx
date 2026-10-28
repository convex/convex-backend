import { CheckCircledIcon, CrossCircledIcon } from "@radix-ui/react-icons";
import { memo, useRef } from "react";
import { FunctionResult } from "convex/browser";
import classNames from "classnames";
import { entryOutput, useLogsForSingleFunction } from "@common/lib/useLogs";
import { LogLinesOutput } from "@common/elements/LogOutput";
import { usePrettyReadonlyCode } from "@common/lib/usePrettyReadonlyCode";
import { Spinner } from "@ui/Spinner";
import { RequestFilter } from "@common/lib/appMetrics";
import { msFormat } from "@common/lib/format";
import { CopyButton } from "@common/elements/CopyButton";
import { LiveTimestampDistance } from "@common/elements/TimestampDistance";

export const Result = memo(ResultImpl);

function ResultImpl({
  result,
  loading = false,
  lastRequestTiming,
  queryStatus,
  requestFilter,
  startCursor,
  onCopiedQueryResult,
}: {
  result?: FunctionResult;
  // If the request is in flight.
  loading?: boolean;
  // How long a mutation or action took.
  lastRequestTiming?: {
    startedAt: number;
    endedAt: number;
  };
  // If this is a query, we might want to display the status of it in the output header.
  queryStatus?: React.ReactNode;
  requestFilter: RequestFilter | null;
  startCursor: number;
  onCopiedQueryResult?: () => void;
}) {
  const {
    loading: isFormattingCode,
    component: readonlyEditor,
    stringValue,
  } = usePrettyReadonlyCode(
    result?.success ? result.value : null,
    "functionResult",
    {
      height: { type: "content" },
      disableLineNumbers: true,
    },
  );

  const errorString = result?.success !== false ? null : result.errorMessage;

  return (
    <div className="flex max-w-full grow flex-col">
      <div className="sticky top-0 z-10 flex items-center gap-4 border-y bg-background-primary px-4 py-2">
        <h5 className="text-xs whitespace-nowrap text-content-secondary">
          Output
        </h5>
        {queryStatus}
        {loading || isFormattingCode ? (
          <div>
            <Spinner />
          </div>
        ) : (
          lastRequestTiming !== undefined &&
          result && (
            <div className="flex items-start gap-2 text-xs text-content-primary">
              {!result.success ? (
                <div className="flex gap-0.5 text-content-errorSecondary">
                  <CrossCircledIcon />
                  error
                </div>
              ) : (
                <div className="flex gap-0.5 text-util-success">
                  <CheckCircledIcon />
                  success
                </div>
              )}{" "}
              {msFormat(
                lastRequestTiming.endedAt - lastRequestTiming.startedAt,
              )}
              <LiveTimestampDistance
                prefix="Ran"
                date={new Date(lastRequestTiming.startedAt)}
              />
            </div>
          )
        )}
        {stringValue && (
          <CopyButton
            text={(result?.success ? stringValue : result?.errorMessage) ?? ""}
            inline
            className="h-4 p-0"
            tip="Copy Result Value"
            disabled={loading || result === undefined}
            onCopied={onCopiedQueryResult}
          />
        )}
      </div>
      <div
        className={classNames(
          "grow overflow-y-auto scrollbar animate-fadeInFromLoading",
        )}
        key={lastRequestTiming?.startedAt}
      >
        {result === undefined && !loading ? (
          <div className="py-2 pl-4 text-sm text-content-secondary italic">
            Run this function to produce a result.
          </div>
        ) : (
          <div className="flex animate-fadeInFromLoading flex-col py-2">
            <div className="flex h-full flex-col gap-3">
              <div className="h-full px-4 pb-2 font-mono text-xs">
                {result !== undefined && result.logLines.length !== 0 ? (
                  <LogLinesOutput
                    output={entryOutput({
                      logLines: result.logLines,
                      error: errorString,
                    })}
                  />
                ) : (
                  <LiveLogs
                    requestFilter={requestFilter ?? "skip"}
                    startCursor={startCursor}
                    errorString={errorString}
                  />
                )}
              </div>
            </div>
            <div className="flex w-full max-w-full flex-col gap-1 pl-5 text-sm">
              {result?.success && readonlyEditor}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}

function LiveLogs({
  requestFilter,
  startCursor,
  // errors are shown as a final log line with level "error"
  errorString,
}: {
  requestFilter: RequestFilter | "skip";
  startCursor: number;
  errorString: string | null;
}) {
  const logsConnectivityCallbacks = useRef({
    onReconnected: () => {},
    onDisconnected: () => {},
    onPermissionDenied: () => {},
  });
  const logs = useLogsForSingleFunction(
    logsConnectivityCallbacks.current,
    startCursor,
    requestFilter,
  );
  const logOutputs = entryOutput({ logLines: logs ?? [], error: errorString });
  return <LogLinesOutput output={logOutputs} />;
}
