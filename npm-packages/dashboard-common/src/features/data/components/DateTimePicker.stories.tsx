import { Meta, StoryObj } from "@storybook/nextjs";
import { DateTimePicker } from "@common/features/data/components/DateTimePicker";
import { fn } from "storybook/test";
import { Sheet } from "@ui/Sheet";

const meta = {
  component: DateTimePicker,
  args: {
    onChange: fn(),
    date: new Date("2024-10-07T14:35:32"),
    "aria-label": "Date and time",
  },
  parameters: {
    a11y: { test: "todo" },
  },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof DateTimePicker>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const Disabled: Story = {
  args: {
    disabled: true,
  },
};

export const WithAutoFocus: Story = {
  args: {
    autoFocus: true,
  },
};

export const WithOnSave: Story = {
  args: {
    onSave: fn(),
  },
  parameters: {
    a11y: { test: "todo" },
    docs: {
      description: {
        story: "Press Enter after changing the date to trigger onSave callback",
      },
    },
  },
};

export const WithCustomClassName: Story = {
  args: {
    className: "border-2 border-blue-500 rounded p-2",
  },
};
