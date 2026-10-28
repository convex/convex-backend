import { Meta, StoryObj } from "@storybook/nextjs";
import { Popover } from "@ui/Popover";
import { Button } from "@ui/Button";

const meta = {
  component: Popover,
  args: {
    button: <Button>Open popover</Button>,
    children: (
      <div className="flex flex-col gap-2 text-content-primary">
        <p className="font-semibold">Popover title</p>
        <p className="text-content-secondary">Hello world!</p>
      </div>
    ),
  },
  render: (args) => (
    <div className="flex justify-center p-24">
      <Popover {...args} />
    </div>
  ),
  parameters: {
    a11y: { test: "todo" },
  },
} satisfies Meta<typeof Popover>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {};

export const TopPlacement: Story = {
  args: {
    placement: "top",
  },
};

export const Portaled: Story = {
  args: {
    portal: true,
  },
};
