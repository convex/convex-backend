import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { Checkbox } from "@ui/Checkbox";

const meta = {
  component: Checkbox,
  args: {
    onChange: fn(),
  },
} satisfies Meta<typeof Checkbox>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Unchecked: Story = {
  args: {
    checked: false,
  },
};

export const Checked: Story = {
  args: {
    checked: true,
  },
};

export const Indeterminate: Story = {
  args: {
    checked: "indeterminate",
  },
};

export const DisabledUnchecked: Story = {
  args: {
    checked: false,
    disabled: true,
  },
};

export const DisabledChecked: Story = {
  args: {
    checked: true,
    disabled: true,
  },
};

export const DisabledIndeterminate: Story = {
  args: {
    checked: "indeterminate",
    disabled: true,
  },
};
