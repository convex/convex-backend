import { Meta, StoryObj } from "@storybook/nextjs";
import { mocked, fn } from "storybook/test";
import { useGetCurrentSpend, useCreateSetupIntent } from "api/billing";
import { useStripePaymentSetup, useStripeAddressSetup } from "hooks/useStripe";
import { BillingPage } from "../../pages/t/[team]/settings/billing";

const meta = {
  component: BillingPage,
  parameters: {
    layout: "fullscreen",
    a11y: {
      test: "todo",
    },
  },
  beforeEach: () => {
    mocked(useGetCurrentSpend).mockReturnValue({
      status: "ok",
      data: { totalCents: 1250 },
    });
    mocked(useCreateSetupIntent).mockReturnValue(async () => ({
      clientSecret: "test_secret",
    }));

    const stripeOptions = { clientSecret: "test_secret" } as any;
    mocked(useStripePaymentSetup).mockReturnValue({
      stripePromise: Promise.resolve(null) as any,
      options: stripeOptions,
      resetClientSecret: fn(),
      confirmSetup: fn(async () => ({
        error: undefined,
        paymentMethod: "pm_test",
      })) as any,
    });
    mocked(useStripeAddressSetup).mockReturnValue({
      stripePromise: Promise.resolve(null) as any,
      options: stripeOptions,
    });
  },
} satisfies Meta<typeof BillingPage>;

export default meta;
type Story = StoryObj<typeof meta>;

export const Default: Story = {};
