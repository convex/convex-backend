import { HealthCard } from "@common/elements/HealthCard";
import { useSubscriptionInvalidationsTopK } from "@common/lib/appMetrics";
import { ChartForFunctionRate } from "@common/features/health/components/ChartForFunctionRate";

export function SubscriptionInvalidations() {
  const chartData = useSubscriptionInvalidationsTopK(3);

  return (
    <HealthCard
      title="Subscription Cache Invalidations"
      tip="When a mutation writes data to a table, any query subscribed to the same data in the table will be invalidated. This chart identifies the mutation and table pairs the caused the most queries to be re-run."
    >
      <ChartForFunctionRate
        chartData={chartData}
        kind="subscriptionInvalidations"
      />
    </HealthCard>
  );
}
