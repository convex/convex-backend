import { Meta, StoryObj } from "@storybook/nextjs";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { Sheet } from "@ui/Sheet";

const meta = {
  component: TimestampDistance,
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof TimestampDistance>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    date: new Date("12/19/2022, 10:00:00 AM"),
  },
};
