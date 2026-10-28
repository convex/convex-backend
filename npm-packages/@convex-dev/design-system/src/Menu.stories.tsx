import { Meta, StoryObj } from "@storybook/nextjs";
import { Menu, MenuItem, MenuLink } from "@ui/Menu";

const meta = {
  component: Menu,
  render: (args) => (
    <div className="w-fit">
      <Menu {...args} />
    </div>
  ),
  parameters: {
    a11y: { test: "todo" },
  },
} satisfies Meta<typeof Menu>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    buttonProps: {
      children: "Menu",
      variant: "neutral",
    },
    placement: "bottom",
    children: (
      <>
        <MenuItem action={() => {}} shortcut={["CtrlOrCmd", "C"]}>
          Item 1
        </MenuItem>
        <MenuItem action={() => {}} variant="danger">
          Item 2
        </MenuItem>
        <MenuLink href="/blah" shortcut={["CtrlOrCmd", "O"]}>
          Item 3 (Link)
        </MenuLink>
      </>
    ),
  },
};

export const WithTooltip: Story = {
  args: {
    buttonProps: {
      children: "Menu",
      variant: "neutral",
      tip: "Hello",
    },
    placement: "bottom",
    children: (
      <MenuItem tip="world" action={() => {}} shortcut={["CtrlOrCmd", "C"]}>
        Item 1
      </MenuItem>
    ),
  },
};
