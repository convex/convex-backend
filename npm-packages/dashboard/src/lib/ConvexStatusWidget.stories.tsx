import type { Meta, StoryObj } from "@storybook/nextjs";
import { ConvexStatusWidget } from "./ConvexStatusWidget";
import { Sheet } from "@ui/Sheet";

const meta: Meta<typeof ConvexStatusWidget> = {
  component: ConvexStatusWidget,
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
};

export default meta;
type Story = StoryObj<typeof meta>;

export const AllOperational: Story = {
  args: {
    status: {
      indicator: "none",
      description: "All Systems Operational",
    },
  },
};

export const MinorIssues: Story = {
  args: {
    status: {
      indicator: "minor",
      description: "Minor Service Disruption",
    },
  },
};

export const MajorOutage: Story = {
  args: {
    status: {
      indicator: "major",
      description: "Major Service Outage",
    },
  },
};

export const CriticalOutage: Story = {
  args: {
    status: {
      indicator: "critical",
      description: "Critical System Failure",
    },
  },
};

export const Loading: Story = {
  args: {
    status: undefined,
  },
};
