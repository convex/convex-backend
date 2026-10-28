import { Button } from "@ui/Button";
import { Spinner } from "@ui/Spinner";
import { Stepper } from "@ui/Stepper";
import { TextInput } from "@ui/TextInput";
import React, { useCallback, useEffect, useMemo, useState } from "react";
import { useDebounce } from "react-use";
import { Elements } from "@stripe/react-stripe-js";
import { useGetCoupon, useCreateSubscription } from "api/billing";
import { FormikProvider, useFormik, useFormikContext } from "formik";
import * as Yup from "yup";
import { Address, PlanResponse, TeamResponse } from "generatedApi";
import { PriceSummary } from "components/billing/PriceSummary";
import { usePostHog } from "hooks/usePostHog";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { CardHoldNote, PaymentDetailsForm } from "./PaymentDetailsForm";
import { BillingAddressInputs } from "./BillingAddressInputs";
import { useStripePaymentSetup } from "../../hooks/useStripe";
import { BillingContactInputs } from "./BillingContactInputs";
import {
  SpendingLimits,
  spendingLimitsSchema,
  spendingLimitValueToCents,
} from "./SpendingLimits";
import { UpgradeFormState } from "./upgradeFormState";

export const debounceDurationMs = 200;

export type UpgradePlanContentProps = {
  plan: PlanResponse;
  couponDurationInMonths?: number;
  numMembers: number;
  teamMemberDiscountPct?: number;
  requiresPaymentMethod?: boolean;
  isLoadingPromo?: boolean;
  promoCodeError?: string;
  setPaymentMethod: (paymentMethod?: string) => void;
  billingAddressInputs: React.ReactNode;
  paymentDetailsForm: React.ReactNode;
  teamManagedBy?: string;
};

export const CreateSubscriptionSchema = Yup.object().shape({
  name: Yup.string()
    .min(1, "Billing contact name must be at least 1 character long.")
    .max(128, "Billing contact name must be at most 128 characters long.")
    .required("Billing contact name is required."),
  email: Yup.string()
    .email("Invalid email address.")
    .min(1, "Billing contact name must be at least 1 character long.")
    .required("Billing contact email is required."),
});

export function UpgradePlanContentContainer({
  team,
  email: profileEmail,
  name: profileName,
  onUpgradeComplete,
  plan,
  ...props
}: Pick<UpgradePlanContentProps, "numMembers" | "plan"> & {
  team: TeamResponse;
  email?: string;
  name?: string | null;
  onUpgradeComplete: () => void;
}) {
  const createSubscription = useCreateSubscription(team.id);
  const { capture } = usePostHog();

  const formState = useFormik<UpgradeFormState>({
    initialValues: {
      promoCode: "",
      name: profileName || "",
      email: profileEmail || "",
      planId: plan.id,
      paymentMethod: undefined,
      billingAddress: undefined,
      spendingLimitWarningThresholdUsd: null,
      spendingLimitDisableThresholdUsd: null,
    },
    validationSchema: CreateSubscriptionSchema.concat(
      spendingLimitsSchema({
        // A new billing cycle starts when the user upgrades, so we don’t need to show the
        // warning about setting a spending limit lower than the amount spent in the current
        // billing cycle.
        currentSpending: undefined,
      }),
    ),
    onSubmit: async (v) => {
      await createSubscription({
        planId: v.planId,
        promoCode: v.promoCode || undefined,
        paymentMethod: v.paymentMethod,
        billingAddress: v.billingAddress,
        name: v.name,
        email: v.email,
        ...spendingLimitValueToCents(v),
      });
      if (plan.planType === "CONVEX_PROFESSIONAL") {
        capture("upgraded_to_pro");
      }
      onUpgradeComplete();
    },
  });
  const [debouncedPromoCode, setDebouncedPromoCode] = useState(
    formState.values.promoCode,
  );
  useDebounce(
    () => {
      setDebouncedPromoCode(formState.values.promoCode);
    },
    debounceDurationMs,
    [formState.values.promoCode],
  );

  const couponData = useGetCoupon(team.id, plan.id, debouncedPromoCode || "");

  useEffect(() => {
    void formState.setFieldValue(
      "planId",
      couponData.coupon?.planId || plan.id,
    );
    // Don't need setFieldValue in deps
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [couponData.coupon, plan.id]);

  const setPaymentMethod = useCallback(
    async (method?: string) => {
      await formState.setFieldValue("paymentMethod", method);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const setBillingAddress = useCallback(
    async (address?: Address) => {
      await formState.setFieldValue("billingAddress", address);
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  const { stripePromise, options, confirmSetup, resetClientSecret } =
    useStripePaymentSetup(
      team,
      formState.values.paymentMethod ?? undefined,
      setPaymentMethod,
    );

  return (
    <FormikProvider value={formState}>
      <UpgradePlanContent
        {...props}
        plan={plan}
        teamManagedBy={team.managedBy === "vercel" ? team.managedBy : undefined}
        setPaymentMethod={(p) => {
          if (!p) {
            resetClientSecret();
          }
          void formState.setFieldValue("paymentMethod", p);
        }}
        teamMemberDiscountPct={couponData.coupon?.percentOff}
        requiresPaymentMethod={
          couponData.coupon ? couponData.coupon.requiresPaymentMethod : true
        }
        couponDurationInMonths={
          couponData.coupon?.durationInMonths ?? undefined
        }
        isLoadingPromo={couponData.isLoading}
        promoCodeError={couponData.errorMessage}
        billingAddressInputs={
          options.clientSecret ? (
            <div className="flex flex-col gap-2">
              <h5>Billing Address</h5>
              <Elements stripe={stripePromise} options={options}>
                <BillingAddressInputs onChangeAddress={setBillingAddress} />
              </Elements>
            </div>
          ) : undefined
        }
        paymentDetailsForm={
          // Using dependency injection to pass in the Stripe form
          // so we can test the UpgradePlanContent component in isolation
          options.clientSecret ? (
            <Elements stripe={stripePromise} options={options}>
              <PaymentDetailsForm confirmSetup={confirmSetup} />
            </Elements>
          ) : undefined
        }
      />
    </FormikProvider>
  );
}

export function UpgradePlanContent({
  plan,
  couponDurationInMonths,
  teamMemberDiscountPct = 0,
  numMembers,
  requiresPaymentMethod = true,
  isLoadingPromo = false,
  promoCodeError,
  setPaymentMethod,
  billingAddressInputs,
  paymentDetailsForm,
  teamManagedBy,
}: UpgradePlanContentProps) {
  const formState = useFormikContext<UpgradeFormState>();
  const [currentStep, setCurrentStep] = useState(0);
  // Evaluated by big brain too, which places the hold this note describes.
  const { preAuthorizeStarterPlan } = useLaunchDarkly();

  if (teamMemberDiscountPct < 0 || teamMemberDiscountPct > 1) {
    throw new Error(
      `Invalid teamMemberDiscountPct: ${teamMemberDiscountPct}. Must be between 0 and 1.`,
    );
  }

  const steps = useMemo(
    () => [
      { label: "Billing Information" },
      { label: "Spending Limits" },
      ...(requiresPaymentMethod ? [{ label: "Payment Information" }] : []),
    ],
    [requiresPaymentMethod],
  );

  const totalSteps = steps.length;
  const isLastStep = currentStep === totalSteps - 1;

  // Clamp step if totalSteps decreases (e.g. promo code removes payment requirement)
  useEffect(() => {
    if (currentStep >= totalSteps) {
      setCurrentStep(totalSteps - 1);
    }
  }, [totalSteps, currentStep]);

  const canProceedFromStep0 = useMemo(
    () =>
      !!formState.values.name &&
      !!formState.values.email &&
      !!formState.values.billingAddress &&
      !isLoadingPromo,
    [
      formState.values.name,
      formState.values.email,
      formState.values.billingAddress,
      isLoadingPromo,
    ],
  );

  const stepContent = (index: number) => {
    if (index === 0) {
      return (
        <div className="flex flex-col gap-6">
          <div className="flex flex-col gap-2">
            <h5>Billing Contact</h5>
            <BillingContactInputs formState={formState} />
          </div>

          {billingAddressInputs}

          {plan.planType === "CONVEX_PROFESSIONAL" && (
            <PromoCodeField
              value={formState.values.promoCode}
              onChange={(value) => formState.setFieldValue("promoCode", value)}
              isLoading={isLoadingPromo}
              error={promoCodeError}
            />
          )}
        </div>
      );
    }
    if (index === 1) {
      return (
        <div className="flex flex-col gap-6">
          <div className="flex flex-col gap-2">
            <p className="text-sm text-content-secondary">
              We recommend setting a warning threshold and a disable threshold
              to avoid unexpected charges on your Convex usage.
            </p>
            <SpendingLimits />
            {!requiresPaymentMethod && (
              <p className="text-sm text-content-secondary">
                Payment information is not required for your promotion code, but
                you may add spending limits in case payment is applicable in the
                future.
              </p>
            )}
          </div>
        </div>
      );
    }
    if (index === 2 && requiresPaymentMethod) {
      return (
        <div className="flex flex-col gap-6">
          {preAuthorizeStarterPlan &&
            plan.planType === "CONVEX_STARTER_PLUS" && <CardHoldNote />}
          {!formState.values.paymentMethod ? (
            <div className="flex flex-col gap-2">
              {/* Reduce the amount of space stripe can take up so it doesn't render horizontal overflow */}
              <div className="max-w-[calc(100%-1rem)]">
                {paymentDetailsForm}
              </div>
            </div>
          ) : (
            <Button
              className="w-fit"
              size="sm"
              onClick={() => setPaymentMethod(undefined)}
              data-testid="update-payment-method-button"
              variant="neutral"
            >
              Change payment method
            </Button>
          )}
        </div>
      );
    }
    return null;
  };

  const navigationButtons = (
    <div className="flex gap-2">
      {currentStep > 0 && (
        <Button
          size="sm"
          variant="neutral"
          onClick={() => setCurrentStep(currentStep - 1)}
        >
          Back
        </Button>
      )}
      {!isLastStep ? (
        <Button
          size="sm"
          onClick={() => setCurrentStep(currentStep + 1)}
          disabled={
            (currentStep === 0 && !canProceedFromStep0) ||
            (currentStep === 1 &&
              !!(
                formState.errors.spendingLimitWarningThresholdUsd ||
                formState.errors.spendingLimitDisableThresholdUsd
              ))
          }
          tip={
            currentStep === 0
              ? !formState.values.name
                ? "Enter a billing contact name to continue."
                : !formState.values.email
                  ? "Enter a billing contact email to continue."
                  : !formState.values.billingAddress
                    ? "Enter a billing address to continue."
                    : undefined
              : undefined
          }
        >
          Next
        </Button>
      ) : (
        <form
          onSubmit={async (e) => {
            e.preventDefault();
            await formState.handleSubmit();
          }}
        >
          <Button
            data-testid="upgrade-plan-button"
            className="w-fit"
            size="sm"
            disabled={
              isLoadingPromo ||
              (requiresPaymentMethod && !formState.values.paymentMethod) ||
              !formState.values.billingAddress
            }
            type="submit"
            loading={formState.isSubmitting}
            tip={
              requiresPaymentMethod && !formState.values.paymentMethod
                ? "Add a payment method to continue."
                : !formState.values.name
                  ? "Enter a billing contact name to continue."
                  : !formState.values.email
                    ? "Enter a billing contact email to continue."
                    : !formState.values.billingAddress
                      ? "Enter a billing address to continue."
                      : undefined
            }
          >
            Confirm and Upgrade
          </Button>
        </form>
      )}
    </div>
  );

  return (
    <div className="flex flex-col gap-4">
      <PriceSummary
        plan={plan}
        teamMemberDiscountPct={teamMemberDiscountPct}
        numMembers={numMembers}
        requiresPaymentMethod={requiresPaymentMethod}
        couponDurationInMonths={couponDurationInMonths}
        isUpgrading={false}
        teamManagedBy={teamManagedBy}
      />

      <Stepper activeStep={currentStep} onSelectStep={setCurrentStep}>
        {steps.map((step, index) => (
          <Stepper.Step key={step.label} label={step.label}>
            {stepContent(index)}
            {index === currentStep && navigationButtons}
          </Stepper.Step>
        ))}
      </Stepper>
    </div>
  );
}

function PromoCodeField({
  value,
  onChange,
  isLoading,
  error,
}: {
  value: string | undefined;
  onChange: (value: string) => void;
  isLoading: boolean;
  error?: string;
}) {
  const [isExpanded, setIsExpanded] = useState(() => !!value || !!error);

  if (!isExpanded) {
    return (
      <Button
        variant="unstyled"
        onClick={() => setIsExpanded(true)}
        className="self-start text-xs text-content-tertiary underline hover:text-content-secondary"
      >
        Have a promo code?
      </Button>
    );
  }

  return (
    <div className="flex max-w-64 items-center gap-2">
      <TextInput
        label="Promo code"
        placeholder="Enter a promo code"
        onChange={(e) => onChange(e.target.value.toUpperCase())}
        value={value}
        id="promoCode"
        error={error}
        autoFocus
      />
      {isLoading && (
        <span data-testid="loading-spinner" className="mt-4">
          <Spinner />
        </span>
      )}
    </div>
  );
}
