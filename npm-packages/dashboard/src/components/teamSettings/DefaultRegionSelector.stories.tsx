import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { Sheet } from "@ui/Sheet";
import { DefaultRegionSelector } from "./DefaultRegionSelector";

const meta = {
  component: DefaultRegionSelector,
  args: {
    value: null,
    onChange: fn(),
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
    teamSlug: "example-team",
  },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof DefaultRegionSelector>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const WithSelectedRegion: Story = {
  args: {
    value: "aws-us-east-1",
  },
};

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
  },
};

export const NonAdmin: Story = {
  args: {
    disabledDueToPermissions: true,
  },
};

export const NonAdminEuropeNotAvailable: Story = {
  args: {
    disabledDueToPermissions: true,
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
