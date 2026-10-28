import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { ProvisionDeploymentFormInner } from "./ProvisionDeploymentForm";

const meta = {
  component: ProvisionDeploymentFormInner,
  args: {
    deploymentType: "prod",
    regions: [
      {
        displayName: "US East (N. Virginia)",
        name: "aws-us-east-1",
        available: true,
      },
      {
        displayName: "Europe (Ireland)",
        name: "aws-eu-west-1",
        available: true,
      },
    ],
    expectedRegionCount: 2,
    onCreate: fn(),
    teamSlug: "example-team",
    teamName: "Example Team",
    isAdmin: true,
  },
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof ProvisionDeploymentFormInner>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const EuropeNotAvailable: Story = {
  args: {
    regions: [
      {
        displayName: "US East (N. Virginia)",
        name: "aws-us-east-1",
        available: true,
      },
      {
        displayName: "Europe (Ireland)",
        name: "aws-eu-west-1",
        available: false,
      },
    ],
    teamSlug: "example-team",
  },
};

export const AllRegions: Story = {
  args: {
    regions: [
      {
        displayName: "US East (N. Virginia)",
        name: "aws-us-east-1",
        available: true,
      },
      {
        displayName: "Europe (Ireland)",
        name: "aws-eu-west-1",
        available: true,
      },
      {
        displayName: "Canada (Central)",
        name: "aws-ca-central-1",
        available: true,
      },
      {
        displayName: "Asia Pacific (Sydney)",
        name: "aws-ap-southeast-2",
        available: true,
      },
    ],
    expectedRegionCount: 4,
  },
};

export const Loading: Story = {
  args: {
    regions: undefined,
  },
};

export const Development: Story = {
  args: {
    deploymentType: "dev",
    teamSlug: "example-team",
  },
};

export const NotAdmin: Story = {
  args: {
    isAdmin: false,
  },
};
