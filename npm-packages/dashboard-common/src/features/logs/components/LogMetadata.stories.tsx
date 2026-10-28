import type { Meta, StoryObj } from "@storybook/nextjs";
import { functionIdentifierValue } from "@common/lib/functions/generateFileTree";
import { LogMetadata } from "./LogMetadata";
import { Sheet } from "@ui/Sheet";

const meta: Meta<typeof LogMetadata> = {
  component: LogMetadata,
  parameters: {
    layout: "padded",
    a11y: { test: "todo" },
  },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
};

export default meta;
type Story = StoryObj<typeof meta>;

const mockQueryLogs = [
  {
    id: "1",
    udfType: "Query" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/users:list"),
    cachedResult: false,
    requestId: "req_123",
    executionId: "exec_1",
    caller: "SyncWorker",
    environment: "isolate" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 45.5,
    usageStats: {
      databaseReadBytes: 5120,
      databaseReadDocuments: 25,
      databaseWriteBytes: 0,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
    returnBytes: 2048,
  },
];

const mockMutationLogs = [
  {
    id: "1",
    udfType: "Mutation" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/users:create"),
    cachedResult: false,
    requestId: "req_456",
    executionId: "exec_2",
    caller: "HttpApi",
    environment: "isolate" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 120.3,
    usageStats: {
      databaseReadBytes: 1024,
      databaseReadDocuments: 3,
      databaseWriteBytes: 4096,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
    returnBytes: 512,
  },
];

const mockActionLogs = [
  {
    id: "1",
    udfType: "Action" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/email:send"),
    cachedResult: false,
    requestId: "req_789",
    executionId: "exec_3",
    caller: "Scheduler",
    environment: "node" as const,
    identityType: "system",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 2500,
    usageStats: {
      databaseReadBytes: 2048,
      databaseReadDocuments: 5,
      databaseWriteBytes: 1024,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 10240,
      storageWriteBytes: 5120,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 128,
    },
    returnBytes: 1024,
  },
];

const mockHttpActionLogs = [
  {
    id: "1",
    udfType: "HttpAction" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("http:webhook"),
    cachedResult: false,
    requestId: "req_http_1",
    executionId: "exec_http_1",
    caller: "HttpEndpoint",
    environment: "node" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: "200",
    },
    executionTimeMs: 850,
    usageStats: {
      databaseReadBytes: 8192,
      databaseReadDocuments: 10,
      databaseWriteBytes: 2048,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 64,
    },
    returnBytes: 4096,
  },
];

const mockCachedQueryLogs = [
  {
    id: "1",
    udfType: "Query" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/users:list"),
    cachedResult: true,
    requestId: "req_cached_1",
    executionId: "exec_cached_1",
    caller: "SyncWorker",
    environment: "isolate" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 5.2,
    usageStats: {
      databaseReadBytes: 0,
      databaseReadDocuments: 0,
      databaseWriteBytes: 0,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
    returnBytes: 2048,
  },
];

const mockAdminLogs = [
  {
    id: "1",
    udfType: "Mutation" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("admin/users:delete"),
    cachedResult: false,
    requestId: "req_admin_1",
    executionId: "exec_admin_1",
    caller: "Tester",
    environment: "isolate" as const,
    identityType: "instance_admin",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 75.8,
    usageStats: {
      databaseReadBytes: 512,
      databaseReadDocuments: 1,
      databaseWriteBytes: 512,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
  },
];

const mockLogsWithVectorData = [
  {
    id: "1",
    udfType: "Query" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/search:vectors"),
    cachedResult: false,
    requestId: "req_vector_1",
    executionId: "exec_vector_1",
    caller: "SyncWorker",
    environment: "isolate" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 125.5,
    usageStats: {
      databaseReadBytes: 2048,
      databaseReadDocuments: 5,
      databaseWriteBytes: 0,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 16384,
      vectorIndexWriteBytes: 8192,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
    returnBytes: 8192,
  },
];

const mockMultipleExecutionsLogs = [
  {
    id: "1",
    udfType: "Query" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/users:list"),
    cachedResult: false,
    requestId: "req_multi_1",
    executionId: "exec_multi_1",
    caller: "SyncWorker",
    environment: "isolate" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 45.5,
    usageStats: {
      databaseReadBytes: 5120,
      databaseReadDocuments: 25,
      databaseWriteBytes: 0,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
    returnBytes: 2048,
  },
  {
    id: "2",
    udfType: "Mutation" as const,
    localizedTimestamp: "2023-10-15 10:30:01",
    timestamp: 1697365801000,
    call: functionIdentifierValue("api/users:update"),
    cachedResult: false,
    requestId: "req_multi_1",
    executionId: "exec_multi_2",
    caller: "Action",
    environment: "isolate" as const,
    identityType: "user",
    parentExecutionId: "exec_multi_1",
    executionTimestamp: 1697365801000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 80.2,
    usageStats: {
      databaseReadBytes: 1024,
      databaseReadDocuments: 3,
      databaseWriteBytes: 2048,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 0,
    },
    returnBytes: 512,
  },
  {
    id: "3",
    udfType: "Action" as const,
    localizedTimestamp: "2023-10-15 10:30:02",
    timestamp: 1697365802000,
    call: functionIdentifierValue("api/email:send"),
    cachedResult: false,
    requestId: "req_multi_1",
    executionId: "exec_multi_3",
    caller: "Action",
    environment: "node" as const,
    identityType: "user",
    parentExecutionId: "exec_multi_2",
    executionTimestamp: 1697365802000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 1500,
    usageStats: {
      databaseReadBytes: 0,
      databaseReadDocuments: 0,
      databaseWriteBytes: 0,
      databaseIoReadBytes: 0,
      databaseIoWriteBytes: 0,
      storageReadBytes: 0,
      storageWriteBytes: 0,
      vectorIndexReadBytes: 0,
      vectorIndexWriteBytes: 0,
      textIndexQueryBytes: 0,
      textIndexWriteQueryBytes: 0,
      vectorIndexReadQueryBytes: 0,
      vectorIndexWriteQueryBytes: 0,
      networkEgressBytes: 0,
      memoryUsedMb: 96,
    },
    returnBytes: 256,
  },
];

export const QueryRequest: Story = {
  args: {
    requestId: "req_123",
    logs: mockQueryLogs,
  },
};

export const MutationRequest: Story = {
  args: {
    requestId: "req_456",
    logs: mockMutationLogs,
  },
};

export const ActionRequest: Story = {
  args: {
    requestId: "req_789",
    logs: mockActionLogs,
  },
};

export const HttpActionRequest: Story = {
  args: {
    requestId: "req_http_1",
    logs: mockHttpActionLogs,
  },
};

export const CachedQuery: Story = {
  args: {
    requestId: "req_cached_1",
    logs: mockCachedQueryLogs,
  },
};

export const AdminRequest: Story = {
  args: {
    requestId: "req_admin_1",
    logs: mockAdminLogs,
  },
};

export const WithVectorData: Story = {
  args: {
    requestId: "req_vector_1",
    logs: mockLogsWithVectorData,
  },
};

export const MultipleExecutions: Story = {
  args: {
    requestId: "req_multi_1",
    logs: mockMultipleExecutionsLogs,
  },
};

export const ExecutionView: Story = {
  args: {
    requestId: "req_multi_1",
    logs: mockMultipleExecutionsLogs,
    executionId: "exec_multi_2",
  },
};

const mockAllMetricsLogs = [
  {
    id: "1",
    udfType: "Action" as const,
    localizedTimestamp: "2023-10-15 10:30:00",
    timestamp: 1697365800000,
    call: functionIdentifierValue("api/search:fullPipeline"),
    cachedResult: false,
    requestId: "req_all_metrics",
    executionId: "exec_all_metrics",
    caller: "HttpEndpoint",
    environment: "node" as const,
    identityType: "user",
    parentExecutionId: null,
    executionTimestamp: 1697365800000,
    kind: "outcome" as const,
    outcome: {
      status: "success" as const,
      statusCode: null,
    },
    executionTimeMs: 3200,
    usageStats: {
      databaseReadBytes: 8192,
      databaseReadDocuments: 15,
      databaseWriteBytes: 4096,
      databaseIoReadBytes: 2048,
      databaseIoWriteBytes: 1024,
      storageReadBytes: 20480,
      storageWriteBytes: 10240,
      vectorIndexReadBytes: 16384,
      vectorIndexWriteBytes: 8192,
      textIndexQueryBytes: 4096,
      textIndexWriteQueryBytes: 2048,
      vectorIndexReadQueryBytes: 6144,
      vectorIndexWriteQueryBytes: 3072,
      networkEgressBytes: 32768,
      memoryUsedMb: 256,
    },
    returnBytes: 8192,
  },
];

export const AllMetricsPopulated: Story = {
  args: {
    requestId: "req_all_metrics",
    logs: mockAllMetricsLogs,
  },
};
