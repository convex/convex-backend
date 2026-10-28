import React from "react";
import { Meta, StoryObj } from "@storybook/nextjs";
import { Callout } from "@ui/Callout";
import { Formik } from "formik";
import { Sheet } from "@ui/Sheet";
import { fn } from "storybook/test";
import {
  UpgradePlanContent,
  UpgradePlanContentProps,
} from "./UpgradePlanContent";
import { UpgradeFormState } from "./upgradeFormState";

const DEFAULT_FORM_STATE: UpgradeFormState = {
  email: "",
  name: "",
  planId: "",
  paymentMethod: "abc",
  spendingLimitWarningThresholdUsd: null,
  spendingLimitDisableThresholdUsd: null,
};

const meta = {
  component: UpgradePlanContent,
  render: (args) => render(args, DEFAULT_FORM_STATE),
  args: {
    plan: {
      name: "Professional",
      id: "CONVEX_PROFESSIONAL",
      description: "The professional plan.",
      status: "active",
      seatPrice: 25,
      planType: "CONVEX_PROFESSIONAL",
    },
    numMembers: 2,
    paymentDetailsForm: (
      <Callout variant="localDev" className="w-fit">
        STRIPE PAYMENT DETAILS FORM WOULD BE HERE!
      </Callout>
    ),
    setPaymentMethod: fn(),
    billingAddressInputs: (
      <Callout variant="localDev" className="w-fit">
        Billing address inputs would be here
      </Callout>
    ),
  },
} satisfies Meta<typeof UpgradePlanContent>;

export default meta;
type Story = StoryObj<typeof meta>;

function render(args: UpgradePlanContentProps, formState: UpgradeFormState) {
  return (
    <Sheet>
      <Callout variant="localDev" className="mb-4">
        Inputs do not work in this storybook preview. Change the formState prop
        to see different states.
      </Callout>
      <Formik initialValues={formState} onSubmit={() => {}}>
        <UpgradePlanContent {...args} />
      </Formik>
    </Sheet>
  );
}

export const NoPaymentMethod: Story = {
  args: {},
  render: (args) =>
    render(args, {
      ...DEFAULT_FORM_STATE,
      paymentMethod: undefined,
    }),
};

export const HasPaymentMethod: Story = {
  args: {},
};

export const WithDiscount: Story = {
  args: {
    teamMemberDiscountPct: 0.5,
  },
};

export const WithPhasedDiscount: Story = {
  args: {
    teamMemberDiscountPct: 0.5,
    couponDurationInMonths: 2,
  },
};

export const WithFreeDiscount: Story = {
  args: {
    teamMemberDiscountPct: 1,
  },
};

export const WithFreeDiscountAndNoPaymentMethod: StoryObj<
  typeof UpgradePlanContent
> = {
  render: (args) =>
    render(args, {
      ...DEFAULT_FORM_STATE,
      paymentMethod: undefined,
      promoCode: "TOTALLY_FREE",
    }),
  args: {
    teamMemberDiscountPct: 1,
  },
};

export const LoadingPromo: Story = {
  render: (args) =>
    render(args, {
      ...DEFAULT_FORM_STATE,
      paymentMethod: undefined,
      promoCode: "LOADING",
    }),
  args: {
    isLoadingPromo: true,
  },
};

export const InvalidPromo: Story = {
  render: (args) =>
    render(args, {
      ...DEFAULT_FORM_STATE,
      paymentMethod: undefined,
      promoCode: "INVALID",
    }),
  args: {
    promoCodeError: "Invalid promo code.",
  },
};
