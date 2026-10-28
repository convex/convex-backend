import {
  InfoCircledIcon,
  QuestionMarkCircledIcon,
} from "@radix-ui/react-icons";
import { Insight } from "api/insights";
import { Button } from "@ui/Button";
import { formatNumberCompact } from "@common/lib/format";
import { Link } from "@ui/Link";

export function ProblemForInsight({
  insight,
  explain = false,
}: {
  insight: Insight;
  explain?: boolean;
}) {
  switch (insight.kind) {
    case "bytesReadLimit":
    case "bytesReadThreshold": {
      const bytesReadInsight = insight as Insight & {
        kind: "bytesReadLimit" | "bytesReadThreshold";
      };
      return (
        <BytesReadCountProblem insight={bytesReadInsight} explain={explain} />
      );
    }
    case "documentsReadLimit":
    case "documentsReadThreshold": {
      const docsReadInsight = insight as Insight & {
        kind: "documentsReadLimit" | "documentsReadThreshold";
      };
      return (
        <DocsReadCountProblem insight={docsReadInsight} explain={explain} />
      );
    }
    case "occFailedPermanently":
    case "occRetried": {
      const occInsight = insight as Insight & {
        kind: "occFailedPermanently" | "occRetried";
      };
      return <OCCProblem insight={occInsight} explain={explain} />;
    }
    default: {
      insight satisfies never;
      return null;
    }
  }
}

function OCCProblem({
  insight,
  explain,
}: {
  insight: Insight & {
    kind: "occFailedPermanently" | "occRetried";
  };
  explain: boolean;
}) {
  const isFailed = insight.kind === "occFailedPermanently";
  return (
    <div className="flex flex-col items-start gap-1">
      <span className="flex items-center gap-1 truncate">
        {isFailed ? "Failed" : "Retried"} due to write conflicts{" "}
        {explain && (
          <>
            in table{" "}
            {insight.details.occTableName ? (
              <span className="font-semibold">
                {insight.details.occTableName}
              </span>
            ) : (
              <span className="text-content-secondary">an unknown table</span>
            )}
          </>
        )}
        {!explain && (
          <Button
            href="https://docs.convex.dev/error#1"
            tip={
              <>
                <Link href="https://docs.convex.dev/error#1" target="_blank">
                  Learn more
                </Link>{" "}
                about write conflicts.
              </>
            }
            tipSide="right"
            variant="unstyled"
            target="_blank"
          >
            <QuestionMarkCircledIcon />
          </Button>
        )}
      </span>
      {!explain && (
        <span className="max-w-full truncate text-left text-xs text-content-secondary">
          {formatNumberCompact(insight.details.occCalls)} time
          {insight.details.occCalls === 1 ? "" : "s"} in{" "}
          {!insight.details.occTableName ? (
            "an unknown table"
          ) : (
            <>
              table{" "}
              <span className="font-semibold">
                {insight.details.occTableName}
              </span>
            </>
          )}
        </span>
      )}
      {explain && (
        <div className="my-2 flex max-w-prose items-start gap-2 rounded-sm border p-2 text-sm text-pretty">
          <InfoCircledIcon className="mt-[3px] shrink-0" />
          <div className="space-y-2">
            <p>
              Write conflicts occur when two functions running in parallel make
              conflicting changes to the same table.{" "}
            </p>
            <p>
              Convex will attempt to retry mutations that fail this way, but
              will eventually fail permanently if the conflicts persist.
            </p>
            <p>
              <Link href="https://docs.convex.dev/error#1" target="_blank">
                Learn how to debug this Insight.
              </Link>
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

function BytesReadCountProblem({
  insight,
  explain,
}: {
  insight: Insight & { kind: "bytesReadLimit" | "bytesReadThreshold" };
  explain: boolean;
}) {
  return (
    <div className="flex flex-col items-start gap-1">
      <ReadLimitProblem explain={explain} kind="bytes" />
      {!explain && (
        <span className="text-xs text-content-secondary">
          {formatNumberCompact(insight.details.count)} function call
          {insight.details.count === 1 ? "" : "s"}
        </span>
      )}
    </div>
  );
}

export const documentsReadLimit = 32000;
export const megabytesReadLimit = 16;

const bytesLimitString = `${megabytesReadLimit} MB`;
const documentsLimitString = documentsReadLimit.toLocaleString();

function ReadLimitProblem({
  kind,
  explain,
}: {
  kind: "bytes" | "documents";
  explain: boolean;
}) {
  return (
    <div className="flex flex-col">
      <span className="flex items-center gap-1">
        Nearing {kind} read limit
        {!explain && (
          <Button
            href="https://stack.convex.dev/queries-that-scale"
            tip={`This function has been approaching or exceeding the Convex limit on ${kind} read. When a function exceeds the limit of ${kind === "bytes" ? bytesLimitString : documentsLimitString}, it will fail.`}
            tipSide="right"
            variant="unstyled"
            target="_blank"
          >
            <QuestionMarkCircledIcon />
          </Button>
        )}
      </span>
      {explain && (
        <div className="my-2 flex max-w-prose items-start gap-2 rounded-sm border p-2 text-sm text-pretty">
          <InfoCircledIcon className="mt-[3px] shrink-0" />
          <div className="space-y-2">
            <p>
              This issue occurs when a function gets close to or exceeds the
              limit on {kind} read within a single transaction.{" "}
            </p>
            <p>
              When a function exceeds the{" "}
              <Link
                href="https://docs.convex.dev/production/state/limits#transactions"
                target="_blank"
              >
                limit of{" "}
                {kind === "bytes" ? bytesLimitString : documentsLimitString}
              </Link>
              , it will fail.
            </p>
            <p>
              To limit the amount of data read by a function, consider{" "}
              <Link
                href="https://docs.convex.dev/database/indexes/indexes-and-query-perf"
                target="_blank"
              >
                adding an index
              </Link>{" "}
              or{" "}
              <Link
                href="https://docs.convex.dev/database/pagination"
                target="_blank"
              >
                implementing pagination
              </Link>
              .
            </p>
          </div>
        </div>
      )}
    </div>
  );
}

function DocsReadCountProblem({
  insight,
  explain,
}: {
  insight: Insight & {
    kind: "documentsReadLimit" | "documentsReadThreshold";
  };
  explain: boolean;
}) {
  return (
    <div className="flex flex-col items-start gap-1">
      <ReadLimitProblem explain={explain} kind="documents" />
      {!explain && (
        <span className="text-xs text-content-secondary">
          {formatNumberCompact(insight.details.count)} function call
          {insight.details.count === 1 ? "" : "s"}
        </span>
      )}
    </div>
  );
}
