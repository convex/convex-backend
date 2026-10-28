import { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { CheckboxLine } from "./LoginTerms";

const meta = {
  component: CheckboxLine,
  args: {
    optInName: "tos",
    toggle: fn(),
  },
} satisfies Meta<typeof CheckboxLine>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};
