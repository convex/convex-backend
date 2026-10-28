import type { Meta, StoryObj } from "@storybook/nextjs";
import { mocked } from "storybook/test";
import { MemberResponse } from "generatedApi";
import { useProjectById } from "api/projects";
import { DeploymentRow } from "./DeploymentRow";

const teamMembers: MemberResponse[] = [
  {
    id: 1,
    name: "Alice Johnson",
    email: "alice@convex.dev",
  },
  {
    id: 2,
    name: "Bob Smith",
    email: "bob@convex.dev",
  },
];

const meta = {
  component: DeploymentRow,
  args: {
    teamSlug: "my-team",
    teamMembers,
  },
  parameters: { a11y: { test: "todo" } },
  beforeEach: () => {
    mocked(useProjectById).mockReturnValue({
      project: {
        id: 1,
        slug: "my-awesome-project",
        name: "My Awesome Project",
        teamId: 1,
        createTime: Date.now() - 30 * 24 * 60 * 60 * 1000,
      },
      isLoading: false,
      error: undefined,
    } as ReturnType<typeof useProjectById>);
  },
} satisfies Meta<typeof DeploymentRow>;

export default meta;
type Story = StoryObj<typeof meta>;

// Production deployment
export const ProductionCloud: Story = {
  args: {
    deployment: {
      kind: "cloud",
      class: "s16",
      name: "happy-animal-123",
      reference: "prod",
      deploymentType: "prod",
      createTime: Date.now() - 2 * 60 * 60 * 1000, // 2 hours ago
      creator: 1,
      id: 100,
      projectId: 1,
      isDefault: true,
      region: "aws-us-east-1",
      deploymentUrl: "https://happy-animal-123.convex.cloud",
    },
  },
};

// Development cloud deployment (mine)
export const DevelopmentCloudMine: Story = {
  args: {
    deployment: {
      kind: "cloud",
      class: "s16",
      name: "clever-otter-456",
      reference: "dev/alice",
      deploymentType: "dev",
      createTime: Date.now() - 30 * 60 * 1000, // 30 minutes ago
      creator: 1,
      id: 101,
      projectId: 1,
      isDefault: true,
      region: "aws-us-east-1",
      deploymentUrl: "https://clever-otter-456.convex.cloud",
    },
  },
};

// Development cloud deployment (teammate's)
export const DevelopmentCloudTeammate: Story = {
  args: {
    deployment: {
      kind: "cloud",
      class: "s16",
      name: "playful-koala-789",
      reference: "dev/bob",
      deploymentType: "dev",
      createTime: Date.now() - 4 * 60 * 60 * 1000, // 4 hours ago
      creator: 2,
      id: 102,
      projectId: 1,
      isDefault: false,
      region: "aws-us-east-1",
      deploymentUrl: "https://playful-koala-789.convex.cloud",
    },
  },
};

// Local development deployment
export const DevelopmentLocal: Story = {
  args: {
    deployment: {
      kind: "local",
      name: "local-dev",
      deploymentType: "dev",
      createTime: Date.now() - 15 * 60 * 1000, // 15 minutes ago
      creator: 1,
      projectId: 1,
      deviceName: "MacBook Pro",
      port: 3210,
      isActive: true,
    },
  },
};

// Preview deployment
export const Preview: Story = {
  args: {
    deployment: {
      kind: "cloud",
      class: "s16",
      name: "magical-owl-201",
      reference: "preview/my-feature",
      deploymentType: "preview",
      createTime: Date.now() - 1 * 60 * 60 * 1000, // 1 hour ago
      creator: 1,
      id: 103,
      projectId: 1,
      isDefault: false,
      region: "aws-us-east-1",
      previewIdentifier: "my-feature",
      deploymentUrl: "https://magical-owl-201.convex.cloud",
    },
  },
};

// Custom deployment
export const Custom: Story = {
  args: {
    deployment: {
      kind: "cloud",
      class: "s16",
      name: "clever-fox-512",
      reference: "staging",
      deploymentType: "custom",
      createTime: Date.now() - 24 * 60 * 60 * 1000, // 1 day ago
      creator: 2,
      id: 104,
      projectId: 1,
      isDefault: false,
      region: "aws-eu-west-1",
      deploymentUrl: "https://clever-fox-512.convex.cloud",
    },
  },
};

// Old deployment (testing time display)
export const OldDeployment: Story = {
  args: {
    deployment: {
      kind: "cloud",
      class: "s16",
      name: "ancient-tortoise-111",
      reference: "production",
      deploymentType: "prod",
      createTime: Date.now() - 365 * 24 * 60 * 60 * 1000, // 1 year ago
      creator: 2,
      id: 107,
      projectId: 1,
      isDefault: true,
      region: "aws-eu-west-1",
      deploymentUrl: "https://ancient-tortoise-111.convex.cloud",
    },
  },
};
