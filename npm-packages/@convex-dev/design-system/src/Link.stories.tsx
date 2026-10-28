import { Meta, StoryObj } from "@storybook/nextjs";
import { Link } from "@ui/Link";

const meta = {
  component: Link,
} satisfies Meta<typeof Link>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    href: "https://convex.dev",
    children: "Click me",
  },
};

export const NoUnderline: Story = {
  args: {
    href: "https://convex.dev",
    children: "Click me",
    noUnderline: true,
  },
  parameters: {
    a11y: {
      config: {
        rules: [{ id: "link-in-text-block", enabled: false }],
      },
    },
  },
};

export const WithExternalIcon: Story = {
  args: {
    href: "https://convex.dev",
    target: "_blank",
    rel: "noopener noreferrer",
    externalIcon: true,
    children: "Open in new tab",
  },
};
