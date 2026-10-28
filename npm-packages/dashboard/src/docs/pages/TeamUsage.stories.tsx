import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked } from "storybook/test";
import {
  useUsageTeamDocumentsPerDayByProject,
  useUsageTeamDeploymentCountPerDayByProject,
  useUsageTeamDeploymentCountByType,
} from "hooks/usageMetrics";
import {
  useUsageTeamSummary,
  useUsageTeamMetricsByFunction,
  useDatabaseStoragePerDayByProjectAndClass,
  useDatabaseStoragePerDayByTable,
  useDocumentCountPerDayByTable,
  useDatabaseIOPerDayByProjectAndClass,
  useFunctionCallsPerDayByProjectAndClass,
  useComputePerDayByProject,
  useFileStoragePerDayByProject,
  useSearchStoragePerDayByProject,
  useDataEgressPerDayByProject,
  useSearchQueriesPerDayByProject,
  useDeploymentsByClassAndRegion,
  useAiGatewayCostPerDayByProject,
} from "hooks/usageMetrics";
import { useCurrentBillingPeriod } from "api/usage";
import { TeamUsagePage } from "../../pages/t/[team]/settings/usage";

const AI_GATEWAY_COST_ROWS = [...Array(14).keys()].map((dayIndex) => ({
  ds: `2026-02-${(dayIndex + 8).toString().padStart(2, "0")}`,
  projectId: 7,
  value: 0.18 + dayIndex * 0.11 + (dayIndex % 3) * 0.07,
}));

const meta = {
  component: TeamUsagePage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    mocked(useCurrentBillingPeriod).mockReturnValue({
      start: "2026-02-01",
      end: "2026-03-01",
    });
    mocked(useUsageTeamDocumentsPerDayByProject).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useUsageTeamDeploymentCountPerDayByProject).mockReturnValue({
      data: [{ ds: "2026-02-28", projectId: 1, value: 3 }],
      error: undefined,
    });
    mocked(useUsageTeamDeploymentCountByType).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useUsageTeamSummary).mockReturnValue({
      data: [
        {
          deploymentClass: "s16",
          region: "aws-us-east-1",
          databaseStorage: 5_368_709_120,
          databaseIO: 5_368_709_120,
          functionCalls: 5_000_000,
          queryMutationCompute: 10,
          actionComputeConvex: 30,
          actionComputeNode: 20,
          fileStorage: 10_737_418_240,
          searchStorage: 107_374_182,
          dataEgress: 5_368_709_120,
          auditLogBandwidth: 268_435_456,
          searchQueries: 500,
          actionComputeUser: 30,
          deploymentCount: 3,
          pausedDeploymentCount: 1,
          idleDeploymentCount: 1,
        },
      ],
      error: undefined,
    });
    mocked(useUsageTeamMetricsByFunction).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useDatabaseStoragePerDayByProjectAndClass).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useDatabaseStoragePerDayByTable).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useDocumentCountPerDayByTable).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useDatabaseIOPerDayByProjectAndClass).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useFunctionCallsPerDayByProjectAndClass).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useComputePerDayByProject).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useFileStoragePerDayByProject).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useSearchStoragePerDayByProject).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useDataEgressPerDayByProject).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useSearchQueriesPerDayByProject).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useDeploymentsByClassAndRegion).mockReturnValue({
      data: undefined,
      error: undefined,
    });
    mocked(useAiGatewayCostPerDayByProject).mockReturnValue({
      data: AI_GATEWAY_COST_ROWS,
      error: undefined,
    });
  },
} satisfies Meta<typeof TeamUsagePage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};

export const AiGateway: Story = {
  name: "AI Gateway",
  parameters: {
    screenshotSelector: '[data-testid="ai-gateway-usage"]',
    nextjs: {
      router: {
        pathname: "/t/[team]/settings/usage",
        query: { team: "acme", section: "aiGatewayCost" },
      },
    },
  },
};
