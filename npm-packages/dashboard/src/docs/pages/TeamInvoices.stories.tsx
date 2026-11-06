import { Meta, StoryObj } from "@storybook/nextjs";
import { TeamInvoicesPage } from "../../pages/t/[team]/settings/invoices";

const meta = {
  component: TeamInvoicesPage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
} satisfies Meta<typeof TeamInvoicesPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
