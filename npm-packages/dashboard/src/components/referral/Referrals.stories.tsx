import type { Meta, StoryObj } from "@storybook/nextjs";
import { ReferralState } from "generatedApi";
import { ReferralsInner } from "./Referrals";

const meta = {
  component: ReferralsInner,
  tags: ["autodocs"],
  args: {
    referralCode: "CONVEX123",
  },
} satisfies Meta<typeof ReferralsInner>;

export default meta;
type Story = StoryObj<typeof meta>;

const mockReferralState: ReferralState = {
  referrals: ["Team A", "Team B"],
  referredBy: null,
};

export const FreePlan: Story = {
  args: {
    isPaidPlan: false,
    referralState: mockReferralState,
  },
};

export const PaidPlan: Story = {
  args: {
    isPaidPlan: true,
    referralState: mockReferralState,
  },
};

export const Loading: Story = {
  args: {
    isPaidPlan: undefined,
    referralState: undefined,
  },
};
