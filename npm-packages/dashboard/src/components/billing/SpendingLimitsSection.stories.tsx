import { fn } from "storybook/test";
import { Meta, StoryObj } from "@storybook/nextjs";
import { Sheet } from "@ui/Sheet";
import { SpendingLimitsSection } from "./SubscriptionOverview";

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
} satisfies Meta<typeof SpendingLimitsSection>;

export default meta;
type Story = StoryObj<typeof meta>;

export const NoSpendingLimits: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: null,
      warningThresholdCents: null,
      state: null,
    },
    currentSpend: { totalCents: 5000, nextBillingPeriodStart: "2025-09-25" },
  },
};

export const Loading: Story = {
  args: {
    currentSpendLimit: undefined,
    currentSpend: undefined,
  },
};

export const SomeSpendingLimits: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: 10000,
      warningThresholdCents: 8000,
      state: "Running",
    },
    currentSpend: { totalCents: 5000, nextBillingPeriodStart: "2025-09-25" },
  },
};

export const ZeroSpendingLimits: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: 0,
      warningThresholdCents: null,
      state: "Running",
    },
    currentSpend: { totalCents: 0, nextBillingPeriodStart: "2025-09-25" },
  },
};

export const WarningOnly: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: null,
      warningThresholdCents: 100_00,
      state: "Running",
    },
    currentSpend: { totalCents: 0, nextBillingPeriodStart: "2025-09-25" },
  },
};

export const NoAdminPermissions: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: 50_000_00,
      warningThresholdCents: 42_000_00,
      state: "Running",
    },
    currentSpend: { totalCents: 5000, nextBillingPeriodStart: "2025-09-25" },
    hasAdminPermissions: false,
  },
};

export const ExceededSpendingLimit: Story = {
  args: {
    currentSpendLimit: {
      disableThresholdCents: 100_000_00,
      warningThresholdCents: 90_000_00,
      state: "Disabled",
    },
    currentSpend: {
      totalCents: 100_000_00,
      nextBillingPeriodStart: "2025-09-25",
    },
  },
};
