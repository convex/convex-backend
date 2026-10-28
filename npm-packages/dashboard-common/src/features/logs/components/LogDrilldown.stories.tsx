import { Meta, StoryObj } from "@storybook/nextjs";
import { LogDrilldown } from "@common/features/logs/components/LogDrilldown";
import { UdfLog, LogOutcome, UdfLogOutput } from "@common/lib/useLogs";
import { UsageStats } from "system-udfs/convex/_system/frontend/common";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { formatDateTime } from "@common/lib/format";
import { useState } from "react";
import { InterleavedLog } from "../lib/interleaveLogs";

// Helper to convert UdfLog[] to InterleavedLog[]
function toInterleavedLogs(logs: UdfLog[]): InterleavedLog[] {
  return logs.map((log) => ({
    kind: "ExecutionLog" as const,
    executionLog: log,
  }));
}

// Wrapper component that manages log selection state
function LogSelectionWrapper({
  children,
  initialLog,
  logs,
}: {
  children: (props: {
    selectedLog?: InterleavedLog;
    onSelectLog: (log: InterleavedLog) => void;
    onHitBoundary: (boundary: "top" | "bottom" | null) => void;
    onFilterByRequestId?: (requestId: string) => void;
  }) => React.ReactNode;
  initialLog?: InterleavedLog;
  logs: InterleavedLog[];
}) {
  const [selectedLog, setSelectedLog] = useState<InterleavedLog | undefined>(
    initialLog || (logs.length > 0 ? logs[0] : undefined),
  );

  const handleSelectLog = (log: InterleavedLog) => {
    setSelectedLog(log);
  };

  const handleHitBoundary = (_boundary: "top" | "bottom" | null) => {};

  const handleFilterByRequestId = (_requestId: string) => {};

  return (
    <>
      {children({
        selectedLog,
        onSelectLog: handleSelectLog,
        onHitBoundary: handleHitBoundary,
        onFilterByRequestId: handleFilterByRequestId,
      })}
    </>
  );
}

const meta = {
  component: LogDrilldown,
  parameters: {
    layout: "fullscreen",
    a11y: { test: "todo" },
  },
  decorators: [
    (Story) => (
      <div className="h-screen">
        <Story />
      </div>
    ),
  ],
  args: {} as any,
} satisfies Meta<typeof LogDrilldown>;

export default meta;
type Story = StoryObj<typeof meta>;

// Mock data generators
const createMockLogCommon = (
  overrides: Partial<UdfLog> = {},
): Partial<UdfLog> => {
  const timestamp = overrides.timestamp || Date.now();
  return {
    id: "log-123",
    udfType: "Query",
    localizedTimestamp: formatDateTime(new Date(timestamp)),
    timestamp,
    call: functionIdentifierValue("messages:list"),
    requestId: "req-abc123",
    executionId: "exec-def456",
    ...overrides,
  };
};

const createMockOutcomeLog = (overrides: Partial<UdfLog> = {}): UdfLog =>
  ({
    ...createMockLogCommon(overrides),
    kind: "outcome",
    outcome: { status: "success", statusCode: null } as LogOutcome,
    executionTimeMs: 125.5,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    parentExecutionId: null,
    ...overrides,
  }) as UdfLog;

const createMockLogEntry = (overrides: Partial<UdfLog> = {}): UdfLog =>
  ({
    ...createMockLogCommon(overrides),
    kind: "log",
    output: {
      isTruncated: false,
      messages: ["Function executed successfully"],
      timestamp: Date.now(),
      level: "INFO",
    } as UdfLogOutput,
    ...overrides,
  }) as UdfLog;

const mockLogs: UdfLog[] = [
  // Root function logs first
  createMockLogEntry({
    id: "log-1",
    call: functionIdentifierValue("messages:send"),
    executionId: "exec-root-123",
    timestamp: Date.now() - 4500,
    output: {
      isTruncated: false,
      messages: ["Starting message send process"],
      level: "INFO",
    } as UdfLogOutput,
  }),

  // Child function (query) logs
  createMockLogEntry({
    id: "log-2",
    call: functionIdentifierValue("messages:list"),
    executionId: "exec-child-456",
    timestamp: Date.now() - 3900,
    output: {
      isTruncated: false,
      messages: ["Retrieved 5 messages from database"],
      level: "INFO",
    } as UdfLogOutput,
  }),

  // Child function (query) outcome - after its logs
  createMockOutcomeLog({
    id: "log-3",
    call: functionIdentifierValue("messages:list"),
    udfType: "Query",
    executionId: "exec-child-456",
    parentExecutionId: "exec-root-123",
    cachedResult: true,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    timestamp: Date.now() - 3800,
    executionTimeMs: 15.2,
    usageStats: {
      memoryUsedMb: null,
      databaseReadBytes: 1024,
      databaseWriteBytes: 0,
      databaseReadDocuments: 5,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
    } as UsageStats,
  }),

  // Action execution outcome
  createMockOutcomeLog({
    id: "log-4",
    call: functionIdentifierValue("messages:processMessage"),
    udfType: "Action",
    executionId: "exec-action-789",
    parentExecutionId: "exec-root-123",
    caller: "Action",
    environment: "node",
    identityType: "user",
    timestamp: Date.now() - 3000,
    executionTimeMs: 250.8,
    usageStats: {
      memoryUsedMb: 64,
      databaseReadBytes: 512,
      databaseWriteBytes: 256,
      databaseReadDocuments: 2,
      storageReadBytes: 2048,
      storageWriteBytes: 1024,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
    } as UsageStats,
  }),

  // Error case outcome
  createMockOutcomeLog({
    id: "log-5",
    call: functionIdentifierValue("messages:validateInput"),
    udfType: "Mutation",
    executionId: "exec-error-def",
    parentExecutionId: "exec-root-123",
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    timestamp: Date.now() - 2000,
    executionTimeMs: 45.1,
    outcome: { status: "failure", statusCode: null } as LogOutcome,
    error: "ValidationError: Message body cannot be empty",
  }),

  // Root function outcome - last, after all child functions complete
  createMockOutcomeLog({
    id: "log-7",
    call: functionIdentifierValue("messages:send"),
    udfType: "Mutation",
    executionId: "exec-root-123",
    parentExecutionId: null,
    caller: "SyncWorker",
    environment: "isolate",
    identityType: "user",
    timestamp: Date.now() - 1000,
    executionTimeMs: 500.5,
    usageStats: {
      memoryUsedMb: null,
      databaseReadBytes: 2048,
      databaseWriteBytes: 512,
      databaseReadDocuments: 7,
      storageReadBytes: 2048,
      storageWriteBytes: 1024,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
    } as UsageStats,
  }),
];

// Stories
export const Default: Story = {
  render: () => {
    const interleavedLogs = toInterleavedLogs(mockLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-abc123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={mockLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const WithCachedQuery: Story = {
  render: () => {
    const cachedLogs = [
      createMockOutcomeLog({
        id: "cached-1",
        call: functionIdentifierValue("users:getProfile"),
        udfType: "Query",
        executionId: "exec-cached-456",
        parentExecutionId: null,
        cachedResult: true,
        executionTimeMs: 2.1,
        caller: "SyncWorker",
        environment: "isolate",
        identityType: "user",
      }),
      createMockLogEntry({
        id: "cached-2",
        call: functionIdentifierValue("users:getProfile"),
        executionId: "exec-cached-456",
        output: {
          isTruncated: false,
          messages: ["Profile retrieved from cache"],
          level: "INFO",
        } as UdfLogOutput,
      }),
    ];
    const interleavedLogs = toInterleavedLogs(cachedLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-cached-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={cachedLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const WithErrorExecution: Story = {
  render: () => {
    const errorLogs = [
      createMockLogEntry({
        id: "error-2",
        call: functionIdentifierValue("auth:validateToken"),
        executionId: "exec-error-456",
        output: {
          isTruncated: false,
          messages: ["Token validation failed"],
          level: "ERROR",
        } as UdfLogOutput,
      }),
      createMockOutcomeLog({
        id: "error-1",
        call: functionIdentifierValue("auth:validateToken"),
        udfType: "Query",
        executionId: "exec-error-456",
        parentExecutionId: null,
        executionTimeMs: 12.3,
        caller: "SyncWorker",
        environment: "isolate",
        identityType: "user",
        outcome: { status: "failure", statusCode: null } as LogOutcome,
        error: "AuthError: Invalid token signature",
      }),
    ];
    const interleavedLogs = toInterleavedLogs(errorLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-error-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={errorLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const HttpActionExecution: Story = {
  render: () => {
    const httpLogs = [
      createMockOutcomeLog({
        id: "http-1",
        call: functionIdentifierValue("api:uploadFile"),
        udfType: "HttpAction",
        executionId: "exec-http-456",
        parentExecutionId: null,
        executionTimeMs: 1250.7,
        caller: "HttpEndpoint",
        environment: "node",
        identityType: "user",
        outcome: { status: "success", statusCode: "201" } as LogOutcome,
      }),
      createMockLogEntry({
        id: "http-2",
        call: functionIdentifierValue("api:uploadFile"),
        executionId: "exec-http-456",
        output: {
          isTruncated: false,
          messages: ["File uploaded successfully to S3"],
          level: "INFO",
        } as UdfLogOutput,
      }),
    ];
    const interleavedLogs = toInterleavedLogs(httpLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-http-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={httpLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const LongRunningAction: Story = {
  render: () => {
    const longLogs = [
      createMockOutcomeLog({
        id: "long-1",
        call: functionIdentifierValue("background:processLargeDataset"),
        udfType: "Action",
        executionId: "exec-long-456",
        parentExecutionId: null,
        executionTimeMs: 15420.9,
        caller: "Scheduler",
        environment: "node",
        identityType: "system",
      }),
      createMockLogEntry({
        id: "long-2",
        call: functionIdentifierValue("background:processLargeDataset"),
        executionId: "exec-long-456",
        output: {
          isTruncated: false,
          messages: ["Processing 10,000 records..."],
          level: "INFO",
        } as UdfLogOutput,
      }),
      createMockLogEntry({
        id: "long-3",
        call: functionIdentifierValue("background:processLargeDataset"),
        executionId: "exec-long-456",
        output: {
          isTruncated: false,
          messages: ["Completed processing in 15.4 seconds"],
          level: "INFO",
        } as UdfLogOutput,
      }),
    ];
    const interleavedLogs = toInterleavedLogs(longLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-long-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={longLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const MultipleExecutions: Story = {
  render: () => {
    const interleavedLogs = toInterleavedLogs(mockLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-multi-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={mockLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const OverviewMode: Story = {
  render: () => {
    const interleavedLogs = toInterleavedLogs(mockLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-multi-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={mockLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const Paused: Story = {
  render: () => {
    const interleavedLogs = toInterleavedLogs(mockLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-paused-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={mockLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};

export const IncompleteActionExecution: Story = {
  render: () => {
    const incompleteLogs = [
      // Log entries for the running action (no outcome yet)
      createMockLogEntry({
        id: "incomplete-1",
        call: functionIdentifierValue("background:processLargeFile"),
        udfType: "Action",
        executionId: "exec-incomplete-456",
        timestamp: Date.now() - 30000,
        output: {
          isTruncated: false,
          messages: ["Starting file processing..."],
          level: "INFO",
        } as UdfLogOutput,
      }),

      createMockLogEntry({
        id: "incomplete-2",
        call: functionIdentifierValue("background:processLargeFile"),
        udfType: "Action",
        executionId: "exec-incomplete-456",
        timestamp: Date.now() - 25000,
        output: {
          isTruncated: false,
          messages: ["Downloaded file from external API"],
          level: "INFO",
        } as UdfLogOutput,
      }),

      // Completed child function calls within the running action
      createMockOutcomeLog({
        id: "incomplete-3",
        call: functionIdentifierValue("files:validateFormat"),
        udfType: "Query",
        executionId: "exec-child-validation",
        parentExecutionId: "exec-incomplete-456",
        executionTimeMs: 45.2,
        caller: "Action",
        environment: "isolate",
        identityType: "user",
        timestamp: Date.now() - 20000,
      }),

      createMockLogEntry({
        id: "incomplete-4",
        call: functionIdentifierValue("files:validateFormat"),
        executionId: "exec-child-validation",
        timestamp: Date.now() - 19800,
        output: {
          isTruncated: false,
          messages: ["File format validation passed"],
          level: "INFO",
        } as UdfLogOutput,
      }),

      createMockOutcomeLog({
        id: "incomplete-5",
        call: functionIdentifierValue("metadata:extractInfo"),
        udfType: "Mutation",
        executionId: "exec-child-extract",
        parentExecutionId: "exec-incomplete-456",
        executionTimeMs: 125.7,
        caller: "Action",
        environment: "isolate",
        identityType: "user",
        timestamp: Date.now() - 15000,
      }),

      createMockLogEntry({
        id: "incomplete-6",
        call: functionIdentifierValue("metadata:extractInfo"),
        executionId: "exec-child-extract",
        timestamp: Date.now() - 14800,
        output: {
          isTruncated: false,
          messages: ["Extracted metadata and stored in database"],
          level: "INFO",
        } as UdfLogOutput,
      }),

      // Incomplete child function - started but no outcome yet (nested under processLargeFile)
      createMockLogEntry({
        id: "incomplete-child-1",
        call: functionIdentifierValue("storage:uploadChunks"),
        udfType: "Action",
        executionId: "exec-child-upload",
        parentExecutionId: "exec-incomplete-456",
        timestamp: Date.now() - 12000,
        output: {
          isTruncated: false,
          messages: ["Starting batch upload of processed chunks..."],
          level: "INFO",
        } as UdfLogOutput,
      }),

      createMockLogEntry({
        id: "incomplete-child-2",
        call: functionIdentifierValue("storage:uploadChunks"),
        udfType: "Action",
        executionId: "exec-child-upload",
        parentExecutionId: "exec-incomplete-456",
        timestamp: Date.now() - 8000,
        output: {
          isTruncated: false,
          messages: ["Uploaded 3 of 8 chunks to S3..."],
          level: "INFO",
        } as UdfLogOutput,
      }),

      // More recent logs from the still-running action
      createMockLogEntry({
        id: "incomplete-7",
        call: functionIdentifierValue("background:processLargeFile"),
        udfType: "Action",
        executionId: "exec-incomplete-456",
        timestamp: Date.now() - 10000,
        output: {
          isTruncated: false,
          messages: ["Processing chunk 5 of 20..."],
          level: "INFO",
        } as UdfLogOutput,
      }),

      createMockLogEntry({
        id: "incomplete-8",
        call: functionIdentifierValue("background:processLargeFile"),
        udfType: "Action",
        executionId: "exec-incomplete-456",
        timestamp: Date.now() - 5000,
        output: {
          isTruncated: false,
          messages: ["Processing chunk 8 of 20..."],
          level: "INFO",
        } as UdfLogOutput,
      }),

      createMockLogEntry({
        id: "incomplete-9",
        call: functionIdentifierValue("background:processLargeFile"),
        udfType: "Action",
        executionId: "exec-incomplete-456",
        timestamp: Date.now() - 2000,
        output: {
          isTruncated: false,
          messages: ["Processing chunk 12 of 20..."],
          level: "INFO",
        } as UdfLogOutput,
      }),
      // Note: No outcome log for the root action - it's still running
    ];
    const interleavedLogs = toInterleavedLogs(incompleteLogs);
    return (
      <LogSelectionWrapper logs={interleavedLogs}>
        {(navProps) => (
          <LogDrilldown
            requestId="req-incomplete-123"
            shownInterleavedLogs={interleavedLogs}
            allUdfLogs={incompleteLogs}
            onClose={() => {}}
            {...navProps}
            selectedLog={navProps.selectedLog!}
          />
        )}
      </LogSelectionWrapper>
    );
  },
  args: {} as any,
};
