import {
  loadStripe,
  StripeElementsOptions,
  StripeElements,
} from "@stripe/stripe-js";
import { useCreateSetupIntent } from "api/billing";
import { useState, useEffect, useCallback } from "react";
import { TeamResponse } from "generatedApi";
import { useCurrentTheme } from "@common/lib/useCurrentTheme";

// `loadStripe` rejects if the publishable key is missing (e.g. in Storybook,
// tests, or self-hosted builds without billing), so only initialize when it's set.
const stripePromise = process.env.NEXT_PUBLIC_STRIPE_PUBLIC_KEY
  ? loadStripe(process.env.NEXT_PUBLIC_STRIPE_PUBLIC_KEY)
  : Promise.resolve(null);

export function useStripePaymentSetup(
  team: TeamResponse,
  paymentMethod: string | undefined,
  setPaymentMethod: (paymentMethod?: string) => Promise<void>,
  hasAdminPermissions = true,
) {
  const createSetupIntent = useCreateSetupIntent(team.id);
  const [clientSecret, setClientSecret] = useState("");

  useEffect(() => {
    if (!paymentMethod && hasAdminPermissions) {
      setClientSecret("");
      void createSetupIntent().then((data) => {
        setClientSecret(data.clientSecret);
      });
    }
    // dont want to run this effect if createSetupIntent changes
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [paymentMethod, hasAdminPermissions]);

  const currentTheme = useCurrentTheme();
  const prefersDark = currentTheme === "dark";

  // Unfortunately, the Stripe API does not allow for dynamic theming via CSS variables,
  // so we have to hardcode the theme variables and keep them up to date.
  const variables = {
    colorPrimary: "rgb(38, 135, 246)",
    colorBackground: prefersDark ? "rgb(42, 40, 37)" : "rgb(253, 252, 250)",
    colorText: prefersDark ? "rgb(255, 255, 255)" : "rgb(42, 40, 37)",
    colorDanger: prefersDark ? "rgb(254, 76, 65)" : "rgb(238, 52, 47)",
    colorWarning: prefersDark ? "rgb(230, 226, 168)" : "rgb(109, 82, 23)",
    colorSuccess: prefersDark ? "rgb(180, 236, 146)" : "rgb(44, 83, 20)",
    colorTextPlaceholder: prefersDark
      ? "rgb(143, 135, 128)"
      : "rgb(120, 118, 133)",
    fontFamily: `"Inter Variable",
    ui-sans-serif,
    system-ui,
    -apple-system,
    BlinkMacSystemFont,
    "Segoe UI",
    Roboto,
    "Helvetica Neue",
    Arial,
    "Noto Sans",
    sans-serif,
    "Apple Color Emoji",
    "Segoe UI Emoji",
    "Segoe UI Symbol",
    "Noto Color Emoji"`,
    spacingUnit: "0.25rem",
    borderRadius: "0.25rem",
  };

  const borderTransparent = prefersDark
    ? "rgba(163, 156, 148, 0.3)"
    : "rgba(33, 34, 30, 0.14)";

  const borderSelected = prefersDark ? "rgb(225, 215, 205)" : "rgb(30, 28, 25)";

  const options: StripeElementsOptions = {
    clientSecret,
    // TODO: Finish styling to have the Stripe form match the rest of the app
    appearance: {
      theme: "stripe",
      variables,
      rules: {
        ".Label": {
          fontSize: "0.875rem",
        },
        ".Error": {
          fontSize: "0.75rem",
        },
        ".Input": {
          lineHeight: "1.25rem",
          fontSize: "0.875rem",
          boxShadow: "none",
          border: `1px solid ${borderTransparent}`,
          padding: "0.375rem 0.5rem",
        },
        ".Input--invalid": {
          border: `1px solid ${borderTransparent}`,
          color: variables.colorText,
          boxShadow: "none",
        },
        ".Input--invalid:focus": {
          border: `1px solid ${variables.colorDanger}`,
        },
        ".Input:focus": {
          boxShadow: "none",
          border: `1px solid ${borderSelected}`,
        },
      },
    },
  };

  // Validates payment details and saves a payment method in stripe.
  const confirmSetup = useCallback(
    async (elements: StripeElements) => {
      const stripe = await stripePromise;
      if (!stripe) {
        return;
      }
      const { error: submitError } = await elements.submit();
      if (submitError) {
        return { error: submitError.message };
      }
      const { error, setupIntent } = await stripe.confirmSetup({
        elements,
        clientSecret,
        redirect: "if_required",
      });

      if (error) {
        return { error: error.message };
      }
      if (!setupIntent || typeof setupIntent.payment_method !== "string") {
        throw new Error(
          `Unexpected payment method type: ${setupIntent.payment_method}`,
        );
      }

      await setPaymentMethod(setupIntent.payment_method);
      return { error: undefined, paymentMethod: setupIntent.payment_method };
    },
    [clientSecret, setPaymentMethod],
  );

  return {
    stripePromise,
    options,
    resetClientSecret: () => {
      setClientSecret("");
    },
    confirmSetup,
  };
}

export function useStripeAddressSetup(
  team: TeamResponse,
  hasAdminPermissions: boolean,
) {
  // Reuse the existing stripe initialization for collecting the payment method,
  // the main difference is that we don't need the confirmSetup function so we
  // pass in dummy values for paymentMethod and setPaymentMethod.
  const { options } = useStripePaymentSetup(
    team,
    undefined,
    async () => {},
    hasAdminPermissions,
  );
  return {
    stripePromise,
    options,
  };
}
