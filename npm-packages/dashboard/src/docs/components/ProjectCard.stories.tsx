import type { Meta, StoryObj } from "@storybook/nextjs";
import { TeamResponse } from "generatedApi";
import { ProjectResponse } from "@convex-dev/platform/managementApi";
import { ProjectCard } from "components/projects/ProjectCard";
import { useProjectById } from "api/projects";
import { useCurrentTeam } from "api/teams";
import { useHasProjectAdminPermissions } from "api/roles";
import { mocked, userEvent, within } from "storybook/test";

const mockTeam: TeamResponse = {
  id: 2,
  creator: 1,
  slug: "acme",
  name: "Acme Corp",
  suspended: false,
  referralCode: "ACME01",
  referredBy: null,
};

const project: ProjectResponse = {
  id: 7,
  teamId: mockTeam.id,
  teamSlug: mockTeam.slug,
  name: "My amazing app",
  slug: "my-amazing-app",
  createTime: Date.now(),
  prodDeploymentName: "musical-otter-456",
  devDeploymentName: "happy-capybara-123",
};

const meta = {
  component: ProjectCard,
  args: {
    project,
  },
  beforeEach: () => {
    mocked(useCurrentTeam).mockReturnValue(mockTeam);
    mocked(useProjectById).mockReturnValue({
      project,
      isLoading: false,
      error: undefined,
    });
    mocked(useHasProjectAdminPermissions).mockReturnValue(true);
  },
  render: (args) => (
    <div className="w-full max-w-sm">
      <ProjectCard {...args} />
    </div>
  ),
} satisfies Meta<typeof ProjectCard>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  play: async ({ canvasElement }) => {
    const canvas = within(canvasElement);
    await userEvent.click(
      canvas.getByRole("button", { name: "Open project settings" }),
    );
  },
};
