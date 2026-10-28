import {
  CheckCircledIcon,
  CrossCircledIcon,
  SewingPinFilledIcon,
} from "@radix-ui/react-icons";
import { useEffect, useMemo, useRef } from "react";
import { UdfLog, UdfLogOutcome } from "@common/lib/useLogs";
import { FunctionNameOption } from "@common/elements/FunctionNameOption";
import { msFormat } from "@common/lib/format";
import { Spinner } from "@ui/Spinner";
import { cn } from "@ui/cn";

type ExecutionNode = {
  executionId: string;
  functionName: string;
  startTime: number;
  executionTime?: number;
  status: "success" | "failure" | "running";
  parentExecutionId?: string | null;
  caller?: string;
  environment?: string;
  identityType?: string;
  children: ExecutionNode[];
  error?: string;
  udfType: string;
};

export function FunctionCallTree({
  logs,
  currentLog,
}: {
  logs: UdfLog[];
  currentLog: UdfLog;
}) {
  const executionNodes = useMemo(() => createExecutionNodes(logs), [logs]);

  if (executionNodes.length === 0) {
    return null;
  }

  const rootNode = executionNodes[0];

  return (
    <div className="max-h-full animate-fadeInFromLoading p-2 text-xs">
      <p className="pb-2 text-content-tertiary">
        This is an outline of the functions called in this request.
      </p>
      {rootNode && (
        <div>
          {executionNodes.map((node) => (
            <ExecutionTreeNode
              key={node.executionId}
              node={node}
              level={0}
              currentLog={currentLog}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function ExecutionTreeNode({
  node,
  level,
  currentLog,
}: {
  node: ExecutionNode;
  level: number;
  currentLog: UdfLog;
}) {
  const hasChildren = node.children.length > 0;
  const nodeRef = useRef<HTMLDivElement>(null);

  const isCurrent = node.executionId === currentLog.executionId;

  useEffect(() => {
    if (isCurrent && nodeRef.current) {
      nodeRef.current.scrollIntoView({
        block: "nearest",
      });
    }
  }, [isCurrent]);

  return (
    <div className="font-mono text-xs">
      <div
        ref={nodeRef}
        className={cn(
          "flex h-[30px] w-full items-center rounded-md pr-2 pl-4",
          isCurrent && "-ml-px rounded-sm border bg-background-highlight",
        )}
      >
        <div className="flex h-full items-center">
          {level !== 0 &&
            Array.from({ length: level }).map((_, index) => (
              <div
                key={index}
                className="mr-4 h-full w-px shrink-0 bg-border-transparent"
              />
            ))}
        </div>

        <div className="-ml-1.5 flex shrink items-center gap-1">
          {node.status === "running" ? (
            <Spinner className="size-4" />
          ) : node.error ? (
            <CrossCircledIcon
              className="size-4 text-content-error"
              aria-label="Function call failed"
            />
          ) : node.status === "success" ? (
            <CheckCircledIcon
              className="size-4 text-content-success"
              aria-label="Function call succeeded"
            />
          ) : null}

          <FunctionNameOption label={node.functionName} error={!!node.error} />
        </div>
        {node.executionTime && (
          <span className="ml-1 text-content-secondary">
            ({msFormat(node.executionTime)})
          </span>
        )}
        {isCurrent && <SewingPinFilledIcon />}
      </div>

      {hasChildren && (
        <div>
          {node.children.map((child) => (
            <ExecutionTreeNode
              key={child.executionId}
              node={child}
              level={level + 1}
              currentLog={currentLog}
            />
          ))}
        </div>
      )}
    </div>
  );
}

function createCompletedExecutionNodes(logs: UdfLog[]): {
  nodeMap: Map<string, ExecutionNode>;
  outcomeSet: Set<string>;
} {
  const nodeMap = new Map<string, ExecutionNode>();
  const outcomeSet = new Set<string>();

  logs
    .filter((log): log is UdfLog & UdfLogOutcome => log.kind === "outcome")
    .forEach((log) => {
      const node: ExecutionNode = {
        executionId: log.executionId,
        functionName: log.call,
        startTime: log.executionTimestamp || log.timestamp,
        executionTime: log.executionTimeMs ?? undefined,
        status: log.outcome.status,
        parentExecutionId: log.parentExecutionId,
        caller: log.caller,
        environment: log.environment,
        identityType: log.identityType,
        children: [],
        error: log.error,
        udfType: log.udfType,
      };

      nodeMap.set(log.executionId, node);
      outcomeSet.add(log.executionId);
    });

  return { nodeMap, outcomeSet };
}

function createRunningExecutionNodes(
  logs: UdfLog[],
  nodeMap: Map<string, ExecutionNode>,
  outcomeSet: Set<string>,
): void {
  const executionLogMap = new Map<string, UdfLog[]>();
  logs
    .filter((log) => log.kind === "log")
    .forEach((log) => {
      if (!executionLogMap.has(log.executionId)) {
        executionLogMap.set(log.executionId, []);
      }
      executionLogMap.get(log.executionId)!.push(log);
    });

  executionLogMap.forEach((logEntries, executionId) => {
    if (!outcomeSet.has(executionId)) {
      const firstLog = logEntries[0];
      // Try to find parent info from any log entry (might be in overrides)
      const { parentExecutionId } = firstLog as any;

      const node: ExecutionNode = {
        executionId: firstLog.executionId,
        functionName: firstLog.call,
        startTime: firstLog.timestamp,
        executionTime: undefined,
        status: "running",
        parentExecutionId: parentExecutionId || undefined,
        caller: undefined,
        environment: undefined,
        identityType: undefined,
        children: [],
        error: undefined,
        udfType: firstLog.udfType,
      };

      nodeMap.set(executionId, node);
    }
  });
}

function buildExecutionTree(
  nodeMap: Map<string, ExecutionNode>,
): ExecutionNode[] {
  const sortChildren = (node: ExecutionNode) => {
    node.children.sort((a, b) => a.startTime - b.startTime);
    node.children.forEach(sortChildren);
  };

  const rootNodes: ExecutionNode[] = [];
  const allNodes = Array.from(nodeMap.values());

  // Build the tree structure
  allNodes.forEach((node) => {
    if (!node.parentExecutionId) {
      rootNodes.push(node);
    } else {
      const parent = nodeMap.get(node.parentExecutionId);
      if (parent) {
        parent.children.push(node);
      } else {
        rootNodes.push(node);
      }
    }
  });

  rootNodes.sort((a, b) => a.startTime - b.startTime);
  rootNodes.forEach((node) => sortChildren(node));

  return rootNodes;
}

export function createExecutionNodes(logs: UdfLog[]): ExecutionNode[] {
  const { nodeMap, outcomeSet } = createCompletedExecutionNodes(logs);
  createRunningExecutionNodes(logs, nodeMap, outcomeSet);
  return buildExecutionTree(nodeMap);
}
