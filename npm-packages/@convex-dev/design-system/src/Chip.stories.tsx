import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { Chip } from "@ui/Chip";

const meta = { component: Chip } satisfies Meta<typeof Chip>;

export default meta;
type Story = StoryObj<typeof meta>;

export const ReadOnly: Story = {
  args: { children: "production" },
};

export const Removable: Story = {
  args: {
    children: "production",
    onRemove: fn(),
    removeLabel: "Remove tag production",
    removeTip: "Remove tag",
  },
};

export const Truncated: Story = {
  args: {
    children: "a-very-long-label-that-does-not-fit",
    className: "max-w-32",
    onRemove: fn(),
    removeLabel: "Remove a-very-long-label-that-does-not-fit",
  },
};
