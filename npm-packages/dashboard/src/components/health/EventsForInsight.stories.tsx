import { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexProvider } from "convex/react";
import { api } from "system-udfs/convex/_generated/api";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { Insight } from "api/insights";
import { mockConvexReactClient } from "@common/lib/mockConvexReactClient";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { mockDeploymentInfo } from "@common/lib/mockDeploymentInfo";
import { EventsForInsight } from "./EventsForInsight";
import { Sheet } from "@ui/Sheet";

const now = new Date();

const baseOccInsight: Insight = {
  functionId: "myFunction",
  componentPath: null,
  kind: "occRetried",
  details: {
    occCalls: 5,
    occTableName: "users",
    hourlyCounts: [
      {
        hour: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(),
        count: 2,
      },
      {
        hour: new Date(now.getTime() - 1 * 60 * 60 * 1000).toISOString(),
        count: 3,
      },
    ],
    recentEvents: [
      {
        timestamp: new Date(now.getTime() - 30 * 60 * 1000).toISOString(),
        id: "exec1",
        request_id: "req1",
        occ_document_id: "doc123",
        occ_write_source: "otherFunction",
        occ_retry_count: 1,
      },
      {
        timestamp: new Date(now.getTime() - 60 * 60 * 1000).toISOString(),
        id: "exec2",
        request_id: "req2",
        occ_document_id: "doc456",
        occ_write_source: "myFunction",
        occ_retry_count: 2,
      },
    ],
  },
};

const baseMetricsInsight: Insight = {
  functionId: "myFunction",
  componentPath: null,
  kind: "bytesReadLimit",
  details: {
    count: 10,
    hourlyCounts: [
      {
        hour: new Date(now.getTime() - 2 * 60 * 60 * 1000).toISOString(),
        count: 4,
      },
      {
        hour: new Date(now.getTime() - 1 * 60 * 60 * 1000).toISOString(),
        count: 6,
      },
    ],
    recentEvents: [
      {
        timestamp: new Date(now.getTime() - 30 * 60 * 1000).toISOString(),
        id: "exec1",
        request_id: "req1",
        calls: [
          {
            table_name: "users",
            bytes_read: 1024 * 1024 * 5,
            documents_read: 100,
          },
          {
            table_name: "posts",
            bytes_read: 1024 * 1024 * 3,
            documents_read: 50,
          },
        ],
        success: true,
      },
      {
        timestamp: new Date(now.getTime() - 60 * 60 * 1000).toISOString(),
        id: "exec2",
        request_id: "req2",
        calls: [
          {
            table_name: "comments",
            bytes_read: 1024 * 1024 * 10,
            documents_read: 200,
          },
        ],
        success: false,
      },
    ],
  },
};

const mockClient = mockConvexReactClient().registerQueryFake(
  api._system.frontend.components.list,
  () => [
    {
      id: "comp1" as Id<"_components">,
      name: "myComponent",
      path: "myComponent",
      args: {},
      state: "active" as const,
      httpPrefix: null,
    },
  ],
);

const meta = {
  component: EventsForInsight,
  args: {
    insight: baseOccInsight,
  },
  render: (args) => (
    <ConvexProvider client={mockClient}>
      <DeploymentInfoContext.Provider value={mockDeploymentInfo}>
        <Sheet>
          <EventsForInsight {...args} />
        </Sheet>
      </DeploymentInfoContext.Provider>
    </ConvexProvider>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof EventsForInsight>;

export default meta;
type Story = StoryObj<typeof meta>;

export const OccRetried: Story = {
  args: {
    insight: baseOccInsight,
  },
};

export const OccRetriedWithComponentPath: Story = {
  args: {
    insight: {
      ...baseOccInsight,
      componentPath: "myComponent",
    },
  },
};

export const OccRetriedWithManyEvents: Story = {
  args: {
    insight: {
      ...baseOccInsight,
      details: {
        ...baseOccInsight.details,
        recentEvents: Array.from({ length: 10 }, (_, i) => ({
          timestamp: new Date(now.getTime() - i * 30 * 60 * 1000).toISOString(),
          id: `exec${i}`,
          request_id: `req${i}`,
          occ_document_id: `doc${i}`,
          occ_write_source: i % 2 === 0 ? "otherFunction" : "myFunction",
          occ_retry_count: i + 1,
        })),
      },
    },
  },
};

export const OccFailedPermanently: Story = {
  args: {
    insight: {
      ...baseOccInsight,
      kind: "occFailedPermanently",
      details: {
        ...baseOccInsight.details,
        recentEvents: [
          {
            timestamp: new Date(now.getTime() - 30 * 60 * 1000).toISOString(),
            id: "exec1",
            request_id: "req1",
            occ_document_id: "doc123",
            occ_write_source: "otherFunction",
            occ_retry_count: 5,
          },
        ],
      },
    },
  },
};

export const OccFailedPermanentlyWithoutTableName: Story = {
  args: {
    insight: {
      ...baseOccInsight,
      kind: "occFailedPermanently",
      details: {
        ...baseOccInsight.details,
        occTableName: undefined,
        recentEvents: [
          {
            timestamp: new Date(now.getTime() - 30 * 60 * 1000).toISOString(),
            id: "exec1",
            request_id: "req1",
            occ_document_id: "doc123",
            occ_write_source: "otherFunction",
            occ_retry_count: 5,
          },
        ],
      },
    },
  },
};

export const BytesReadLimit: Story = {
  args: {
    insight: baseMetricsInsight,
  },
};

export const BytesReadThreshold: Story = {
  args: {
    insight: {
      ...baseMetricsInsight,
      kind: "bytesReadThreshold",
    },
  },
};

export const BytesReadWithManyTables: Story = {
  args: {
    insight: {
      ...baseMetricsInsight,
      details: {
        ...baseMetricsInsight.details,
        recentEvents: [
          {
            timestamp: new Date(now.getTime() - 30 * 60 * 1000).toISOString(),
            id: "exec1",
            request_id: "req1",
            calls: [
              {
                table_name: "users",
                bytes_read: 1024 * 1024 * 2,
                documents_read: 50,
              },
              {
                table_name: "posts",
                bytes_read: 1024 * 1024 * 3,
                documents_read: 75,
              },
              {
                table_name: "comments",
                bytes_read: 1024 * 1024 * 1,
                documents_read: 25,
              },
              {
                table_name: "likes",
                bytes_read: 1024 * 1024 * 4,
                documents_read: 100,
              },
            ],
            success: true,
          },
        ],
      },
    },
  },
};

export const DocumentsReadLimit: Story = {
  args: {
    insight: {
      ...baseMetricsInsight,
      kind: "documentsReadLimit",
    },
  },
};

export const DocumentsReadThreshold: Story = {
  args: {
    insight: {
      ...baseMetricsInsight,
      kind: "documentsReadThreshold",
    },
  },
};

export const WithNoEvents: Story = {
  args: {
    insight: {
      ...baseOccInsight,
      details: {
        ...baseOccInsight.details,
        recentEvents: [],
      },
    },
  },
};
