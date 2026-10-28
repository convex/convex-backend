import { fn } from "storybook/test";
import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { SpendingLimitsSection } from "components/billing/SubscriptionOverview";

const meta = {
  component: SpendingLimitsSection,
  args: {
    hasAdminPermissions: true,
    onSubmit: fn(),
  },
  render: (args) => (
    <Sheet>
      <SpendingLimitsSection {...args} />
    </Sheet>
  ),
  parameters: { a11y: { test: "todo" } },
} satisfies Meta<typeof SpendingLimitsSection>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: 10000,
      warningThresholdCents: 8000,
      state: "Running",
    },
    currentSpend: { totalCents: 5000, nextBillingPeriodStart: "2025-09-25" },
  },
};
