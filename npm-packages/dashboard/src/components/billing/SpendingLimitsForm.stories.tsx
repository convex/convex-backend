import type { Meta, StoryObj } from "@storybook/nextjs";
import { fn } from "storybook/test";
import { SpendingLimitsForm } from "./SpendingLimits";
import { Sheet } from "@ui/Sheet";

const currentSpending = {
  totalCents: 0,
  nextBillingPeriodStart: "2025-09-25",
} as const;

const meta = {
  component: SpendingLimitsForm,
  args: {
    onSubmit: fn(),
    onCancel: fn(),
    currentSpending,
  },
  decorators: [
    (Story) => (
      <Sheet>
        <Story />
      </Sheet>
    ),
  ],
} satisfies Meta<typeof SpendingLimitsForm>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: "",
      spendingLimitDisableThresholdUsd: null,
    },
  },
};

export const BothThresholdsDisabled: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: null,
      spendingLimitDisableThresholdUsd: null,
    },
  },
};

export const BothThresholdsEmpty: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: "",
      spendingLimitDisableThresholdUsd: "",
    },
  },
};

export const DisableThresholdOnly: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: null,
      spendingLimitDisableThresholdUsd: 100,
    },
  },
};

export const WarningThresholdOnly: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: 100,
      spendingLimitDisableThresholdUsd: null,
    },
  },
};

export const HighCurrentSpending: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: null,
      spendingLimitDisableThresholdUsd: "",
    },
    currentSpending: {
      ...currentSpending,
      totalCents: 123_456_78,
    },
  },
};

export const ZeroUsageSpending: Story = {
  args: {
    defaultValue: {
      spendingLimitWarningThresholdUsd: null,
      spendingLimitDisableThresholdUsd: 0,
    },
  },
};

export const Loading: Story = {
  args: {
    defaultValue: undefined,
  },
};
