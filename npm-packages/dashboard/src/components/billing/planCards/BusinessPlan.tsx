import { Button } from "@ui/Button";
import { OrbSubscriptionResponse } from "generatedApi";
import { PlanCard } from "./PlanCard";

export function BusinessPlan({
  subscription,
  isLoading = false,
}: {
  subscription?: OrbSubscriptionResponse;
  isLoading?: boolean;
}) {
  const isCurrentPlan = subscription?.plan.planType === "CONVEX_BUSINESS";

  return (
    <PlanCard
      selected={isCurrentPlan}
      plan={{
        id: "CONVEX_BUSINESS",
        planType: "CONVEX_BUSINESS",
        name: "Business",
        description: "For teams that need custom limits and priority support.",
        status: "active",
        seatPrice: null,
      }}
      saleHeader="$2,500 monthly minimum"
      action={
        isCurrentPlan ? (
          <p className="flex h-8.5 items-center px-2 font-semibold">
            Current Plan
          </p>
        ) : (
          <Button
            variant="neutral"
            disabled={isLoading}
            href="https://www.convex.dev/enterprise/pricing"
            target="_blank"
          >
            See Pricing
          </Button>
        )
      }
    />
  );
}
