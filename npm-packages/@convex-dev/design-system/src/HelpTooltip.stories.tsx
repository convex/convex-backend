import { Meta, StoryObj } from "@storybook/nextjs";
import { HelpTooltip } from "./HelpTooltip";

const meta = {
  component: HelpTooltip,
} satisfies Meta<typeof HelpTooltip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    children: "This is a helpful tip",
  },
};
