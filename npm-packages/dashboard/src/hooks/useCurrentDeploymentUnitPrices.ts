import type { UnitPrice } from "@common/lib/deploymentContext";
import { useTeamPrices } from "api/billing";
import { useCurrentDeployment } from "api/deployments";
import { useCurrentTeam } from "api/teams";
import {
  getPricingPeriodAt,
  getUsagePricing,
} from "components/billing/lib/usagePrices";
import { getDeploymentUnitPrices } from "components/billing/lib/usageSpend";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";

// The current cloud deployment's undiscounted price per unit of each metric,
// keyed by metric name, fetched only when `enabled`. `unitPrices` is undefined
// while loading (`isLoading`) and whenever prices aren't available.
export function useCurrentDeploymentUnitPrices(enabled: boolean): {
  unitPrices: Record<string, UnitPrice> | undefined;
  isLoading: boolean;
} {
  const { dashboardShowCashUsage } = useLaunchDarkly();
  const team = useCurrentTeam();
  const deployment = useCurrentDeployment();
  const isCloud = deployment?.kind === "cloud";
  const today = new Date().toISOString().slice(0, 10);
  const range = enabled && isCloud ? { from: today, to: today } : null;
  const prices = useTeamPrices(team?.id, range);
  if (
    !enabled ||
    !dashboardShowCashUsage ||
    (deployment !== undefined && !isCloud)
  ) {
    return { unitPrices: undefined, isLoading: false };
  }
  if (!isCloud) {
    return { unitPrices: undefined, isLoading: true };
  }
  const pricing = getUsagePricing(prices, range);
  if (pricing.status === "loading") {
    return { unitPrices: undefined, isLoading: true };
  }
  if (
    pricing.status === "error" ||
    (pricing.status === "unavailable" && pricing.reason === "permission")
  ) {
    return { unitPrices: undefined, isLoading: false };
  }
  // `getUsagePricing` only supplies periods aligned to whole days, so take the
  // period in effect now to keep estimates through a mid-day price change.
  const period = prices.data && getPricingPeriodAt(prices.data, Date.now());
  if (!period) {
    return { unitPrices: undefined, isLoading: false };
  }
  return {
    unitPrices: getDeploymentUnitPrices(period, {
      deploymentClass: deployment.class,
      region: deployment.region,
    }),
    isLoading: false,
  };
}
