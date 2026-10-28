import type { Meta, StoryObj } from "@storybook/nextjs";
import { mocked } from "storybook/test";
import { DailyPerTagMetricsByProjectAndClass } from "hooks/usageMetrics";
import { useFunctionCallsPerDayByProjectAndClass } from "hooks/usageMetrics";
import { useProjectById } from "api/projects";
import { FunctionCallsUsage } from "components/billing/TeamUsage";

const rows: DailyPerTagMetricsByProjectAndClass[] = [...Array(14).keys()].map(
  (dayIndex) => {
    const ds = `2026-02-${(dayIndex + 1).toString().padStart(2, "0")}`;
    return {
      ds,
      projectId: 1 as number | "_rest",
      deploymentClass: "s16",
      metrics: [
        { tag: "query", value: (dayIndex + 1) * 80_000 },
        { tag: "mutation", value: (dayIndex + 1) * 30_000 },
        { tag: "action", value: (dayIndex + 1) * 10_000 },
      ],
    };
  },
);

const rows2: DailyPerTagMetricsByProjectAndClass[] = [...Array(14).keys()].map(
  (dayIndex) => {
    const ds = `2026-02-${(dayIndex + 1).toString().padStart(2, "0")}`;
    return {
      ds,
      projectId: 2 as number | "_rest",
      deploymentClass: "s16",
      metrics: [
        { tag: "query", value: (14 - dayIndex) * 50_000 },
        { tag: "mutation", value: (14 - dayIndex) * 20_000 },
        { tag: "action", value: (14 - dayIndex) * 10_000 },
      ],
    };
  },
);

const team = {
  id: 2,
  name: "Acme Corp",
  creator: 1,
  slug: "acme",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

const meta = {
  component: FunctionCallsUsage,
  args: {
    team,
    dateRange: { from: "2026-02-01", to: "2026-03-01" },
    projectId: null,
    componentPrefix: null,
  },
  beforeEach: () => {
    mocked(useFunctionCallsPerDayByProjectAndClass).mockReturnValue({
      data: [...rows, ...rows2],
      error: undefined,
    });
    mocked(useProjectById).mockImplementation((projectId) => ({
      project:
        projectId === 1
          ? {
              id: 1,
              teamId: 2,
              teamSlug: "my-team",
              name: "my-cute-app",
              slug: "my-cute-app",
              createTime: Date.now(),
              prodDeploymentName: "musical-otter-456",
              devDeploymentName: "happy-capybara-123",
            }
          : projectId === 2
            ? {
                id: 2,
                teamId: 2,
                teamSlug: "my-team",
                name: "my-cool-app",
                slug: "my-cool-app",
                createTime: Date.now(),
                prodDeploymentName: "clever-fox-789",
                devDeploymentName: "swift-penguin-012",
              }
            : undefined,
      isLoading: false,
      error: undefined,
    }));
  },
} satisfies Meta<typeof FunctionCallsUsage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
