import type {
  GetPricesPeriodResponse as PricesPeriod,
  GetPricesResponse,
} from "generatedApi";

export type UsagePricingError = {
  message: string;
  code?: string;
};

export type UsagePricing =
  | { status: "loading" }
  | { status: "error"; error: UsagePricingError }
  | {
      status: "unavailable";
      reason: "permission" | "unsupported-period";
    }
  | { status: "ready"; period: PricesPeriod };

export function getUsagePricing(
  {
    data,
    error,
    canView,
  }: {
    data: GetPricesResponse | undefined;
    error: UsagePricingError | undefined;
    canView: boolean | undefined;
  },
  range: { from: string; to: string } | null,
): UsagePricing {
  if (canView === false) {
    return { status: "unavailable", reason: "permission" };
  }
  if (canView === undefined) return { status: "loading" };
  if (error) return { status: "error", error };
  if (!data) return { status: "loading" };

  const period = getUsagePricingPeriod(data, range);
  return period
    ? { status: "ready", period }
    : { status: "unavailable", reason: "unsupported-period" };
}

type PricingPeriod = {
  start: number;
  end: number | null;
};

export function getUsagePricingPeriod<T extends PricingPeriod>(
  prices: { periods: T[] } | undefined,
  range: { from: string; to: string } | null,
): T | undefined {
  if (!prices || !range) return undefined;
  const start = Date.parse(range.from);
  const end = Date.parse(range.to);
  if (!Number.isFinite(start) || !Number.isFinite(end) || start > end) {
    return undefined;
  }
  return prices.periods.find(
    (period) =>
      start >= period.start && (period.end === null || end <= period.end),
  );
}
