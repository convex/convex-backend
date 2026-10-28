import React, { useState } from "react";
import {
  PaymentElement,
  useElements,
  useStripe,
} from "@stripe/react-stripe-js";
import { StripePaymentElementOptions, StripeElements } from "@stripe/stripe-js";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";

export function PaymentDetailsForm({
  confirmSetup,
  showCardHoldNote = false,
}: {
  confirmSetup: (
    elements: StripeElements,
  ) => Promise<{ error?: string; paymentMethod?: string } | undefined>;
  showCardHoldNote?: boolean;
}) {
  const stripe = useStripe();
  const elements = useElements();

  const [isLoading, setIsLoading] = useState(false);

  const paymentElementOptions: StripePaymentElementOptions = {
    layout: "tabs",
  };

  const [error, setError] = useState<string>();

  return (
    <form
      className="flex w-full flex-col gap-4"
      onSubmit={async (e) => {
        e.preventDefault();
        if (!elements) {
          return;
        }
        setIsLoading(true);
        const confirmResult = await confirmSetup(elements);
        if (confirmResult === undefined) {
          setIsLoading(false);
          return;
        }
        if (confirmResult.error) {
          setError(confirmResult.error);
        }
        setIsLoading(false);
      }}
    >
      <PaymentElement
        options={paymentElementOptions}
        onChange={() => error !== undefined && setError(undefined)}
      />
      <Button
        type="submit"
        className="w-fit"
        disabled={!stripe || !elements}
        loading={isLoading}
        size="sm"
      >
        Save payment method
      </Button>
      {showCardHoldNote && <CardHoldNote className="text-xs" />}
      {error && (
        <p className="h-fit text-xs text-content-errorSecondary" role="alert">
          {error}
        </p>
      )}
    </form>
  );
}

export function CardHoldNote({ className }: { className?: string }) {
  return (
    <p className={cn("text-sm text-content-secondary", className)}>
      We’ll place a temporary $10 hold on your card to verify it. Your bank may
      take a few days to remove it.
    </p>
  );
}
