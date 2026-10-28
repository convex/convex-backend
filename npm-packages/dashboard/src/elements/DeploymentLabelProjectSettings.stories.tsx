import { DeploymentLabelProjectSettings } from "./DeploymentDisplay";
import { type Meta, type StoryObj } from "@storybook/nextjs";
import { ProjectDetails } from "generatedApi";

const mockProject: ProjectDetails = {
  id: 1,
  name: "Test Project",
  slug: "test-project",
  teamId: 1,
  createTime: Date.now(),
};

const meta = {
  component: DeploymentLabelProjectSettings,
} satisfies Meta<typeof DeploymentLabelProjectSettings>;

export default meta;
type Story = StoryObj<typeof DeploymentLabelProjectSettings>;

export const Default: Story = {
  args: {
    currentProject: mockProject,
  },
};
