import { DeploymentLabelProvisionDeployment } from "./DeploymentDisplay";
import { type Meta, type StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { ProjectDetails } from "generatedApi";

const mockProject: ProjectDetails = {
  id: 1,
  name: "Test Project",
  slug: "test-project",
  teamId: 1,
  createTime: Date.now(),
};

const meta = {
  component: DeploymentLabelProvisionDeployment,
  args: {
    currentProject: mockProject,
  },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof DeploymentLabelProvisionDeployment>;

export default meta;
type Story = StoryObj<typeof DeploymentLabelProvisionDeployment>;

export const Dev: Story = {
  args: {
    isProvisionProd: false,
  },
};

export const Prod: Story = {
  args: {
    isProvisionProd: true,
  },
};
