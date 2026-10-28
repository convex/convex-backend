import type { Meta, StoryObj } from "@storybook/nextjs";
import { RadioGroup } from "@headlessui/react";
import { Region } from "./Region";

const meta = {
  component: Region,
  render: (args) => (
    <RadioGroup defaultValue="aws-us-east-1">
      <Region {...args} />
    </RadioGroup>
  ),
  args: {
    teamSlug: "example-team",
  },
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof Region>;

export default meta;
type Story = StoryObj<typeof meta>;

export const USEast: Story = {
  args: {
    region: {
      displayName: "US East (N. Virginia)",
      name: "aws-us-east-1",
      available: true,
    },
  },
};

export const Europe: Story = {
  args: {
    region: {
      displayName: "Europe (Ireland)",
      name: "aws-eu-west-1",
      available: true,
    },
  },
};

export const Canada: Story = {
  args: {
    region: {
      displayName: "Canada (Central)",
      name: "aws-ca-central-1",
      available: true,
    },
  },
};

export const Australia: Story = {
  args: {
    region: {
      displayName: "Asia Pacific (Sydney)",
      name: "aws-ap-southeast-2",
      available: true,
    },
  },
};

export const UnavailableRegion: Story = {
  args: {
    region: {
      displayName: "Europe (Ireland)",
      name: "aws-eu-west-1",
      available: false,
    },
  },
};

export const AskEveryTime: Story = {
  args: {
    region: null,
  },
};

export const Loading: Story = {
  args: {
    region: undefined,
  },
};

export const DisabledDueToPermissions: Story = {
  args: {
    region: {
      displayName: "US East (N. Virginia)",
      name: "aws-us-east-1",
      available: true,
    },
    disabledDueToPermissions: true,
  },
};
