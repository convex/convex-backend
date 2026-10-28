import { Meta, StoryObj } from "@storybook/nextjs";
import { TeamResponse } from "generatedApi";
import { fn } from "storybook/test";
import { PromoCodeModal } from "./PromoCodeModal";

const teams: TeamResponse[] = [
  {
    id: 1,
    name: "Acme Inc.",
    slug: "acme",
    creator: 1,
    suspended: false,
    referralCode: "ACME123",
  },
  {
    id: 2,
    name: "Personal projects",
    slug: "personal",
    creator: 1,
    suspended: false,
    referralCode: "PERSONAL123",
  },
];

const promo = {
  code: "3aebea4c-6c93-4cfc-96c1-017089da23e8",
  description: "Hack Night SF 2026",
  creditAmount: 25,
  expirationTime: new Date("2026-12-31T00:00:00Z").getTime(),
  creditValidityDays: 90,
};

const meta = {
  component: PromoCodeModal,
  args: {
    promoState: { status: "success", promo },
    teams,
    selectedTeam: teams[0],
    teamPlan: "paid",
    starterUpgradeUrl: "/t/acme/settings/billing?upgradePlan=starter",
    isRedeeming: false,
    onSelectTeam: fn(),
    onRedeem: fn(async () => {}),
    onClose: fn(),
  },
} satisfies Meta<typeof PromoCodeModal>;

export default meta;
type Story = StoryObj<typeof meta>;

export const PaidTeam: Story = {};

export const FreeTeam: Story = {
  args: { teamPlan: "free" },
};

export const LoadingPromo: Story = {
  args: { promoState: { status: "loading" } },
};

export const InvalidPromo: Story = {
  args: {
    promoState: { status: "error", error: "Unknown promo code." },
  },
};

export const LoadingTeams: Story = {
  args: { teams: undefined },
};

export const LoadingTeamPlan: Story = {
  args: { teamPlan: "loading" },
};

export const BillingStatusError: Story = {
  args: { teamPlan: "error" },
};

export const AlreadyRedeemed: Story = {
  args: {
    promoState: {
      status: "error",
      error: "This promo code has already been redeemed.",
    },
  },
};
