import { Decorator, Meta, StoryObj } from "@storybook/nextjs";
import { Calendar } from "@common/elements/Calendar";

export const Single: Story = {
  args: {
    mode: "single",
    selected: new Date(Date.now() - 24 * 60 * 60 * 1000), // Yesterday
  },
};

export const Range: Story = {
  args: {
    mode: "range",
    selected: {
      from: new Date(Date.now() - 24 * 60 * 60 * 1000), // Yesterday
      to: new Date(Date.now() + 24 * 60 * 60 * 1000), // Tomorrow
    },
  },
};

const rangeStart = new Date(Date.now() - 3 * 24 * 60 * 60 * 1000); // 3 days ago
const rangeEnd = new Date(Date.now() + 3 * 24 * 60 * 60 * 1000); // 3 days from now
export const RestrictedRange: Story = {
  args: {
    mode: "single",
    selected: new Date(Date.now() - 24 * 60 * 60 * 1000), // Yesterday
    startMonth: rangeStart,
    endMonth: rangeEnd,
    disabled: {
      before: rangeStart,
      after: rangeEnd,
    },
    beforeStartTooltip: (
      <>
        This is <em>too early</em>!
      </>
    ),
  },
};

const meta = {
  component: Calendar,
  decorators: [
    ((Story) => (
      // The calendar itself has a transparent background, but should generally be
      // placed over `background-secondary` to ensure proper contrast.
      <div className="w-min bg-background-secondary p-2">
        <Story />
      </div>
    )) satisfies Decorator,
  ],
} satisfies Meta<typeof Calendar>;

export default meta;
type Story = StoryObj<typeof meta>;
