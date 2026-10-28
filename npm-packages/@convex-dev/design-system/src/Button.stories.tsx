import { Meta, StoryObj } from "@storybook/nextjs";
import { PlusIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";

const meta = {
  component: Button,
  render: (args: any) => <Button {...args} />,
} satisfies Meta<typeof Button>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Primary: Story = {
  args: {
    variant: "primary",
    icon: <PlusIcon />,
    children: "Button",
  },
};

export const PrimaryFocused: Story = {
  args: {
    variant: "primary",
    focused: true,
    children: "Button",
  },
};

export const Destructive: Story = {
  args: {
    variant: "danger",
    children: "Button",
  },
};

export const DestructiveFocused: Story = {
  args: {
    variant: "danger",
    focused: true,
    children: "Button",
  },
};

export const Disabled: Story = {
  args: {
    disabled: true,
    children: "Button",
  },
};

export const Link: Story = {
  args: {
    href: "https://convex.dev",
    children: "Link button",
  },
};

export const LinkDisabled: Story = {
  args: {
    href: "https://convex.dev",
    disabled: true,
    children: "No link because it's disabled",
  },
};

export const LinkTargetBlack: Story = {
  args: {
    href: "https://convex.dev",
    target: "_blank",
    children: "Open in a new tab",
  },
};

export const LinkOnClick: Story = {
  args: {
    href: "https://convex.dev",
    onClickOfAnchorLink: () => {
      alert("When you dismiss you'll be redirected");
    },
    children: "Redirect after alert",
  },
};

export const LinkTip: Story = {
  args: {
    href: "https://convex.dev",
    children: "Link button",
    tip: "Goes somewhere",
  },
};
