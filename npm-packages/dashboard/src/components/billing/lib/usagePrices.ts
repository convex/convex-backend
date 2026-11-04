import type { DateRange } from "api/usage";
import type {
  GetPricesPeriodResponse as PricesPeriod,
  GetPricesResponse,
} from "generatedApi";

export type UsagePricingError = {
  message: string;
  code?: string;
};

export type UsagePricingPeriod = {
  period: PricesPeriod;
  range: DateRange;
};

export type UsagePricing =
  | { status: "loading" }
  | { status: "error"; error: UsagePricingError }
  | {
      status: "unavailable";
      reason: "permission" | "unsupported-period";
    }
  | { status: "ready"; periods: UsagePricingPeriod[] };

export function isCurrentBillingPeriodRange(
  range: DateRange | null,
  currentBillingPeriod: { start: string; end: string } | undefined,
): boolean {
  return (
    range === null ||
    (range.from === currentBillingPeriod?.start &&
      range.to === currentBillingPeriod?.end)
  );
}

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
  range: DateRange | null,
): UsagePricing {
  if (canView === false) {
    return { status: "unavailable", reason: "permission" };
  }
  if (canView === undefined) return { status: "loading" };
  if (error) return { status: "error", error };
  if (!data) return { status: "loading" };

  const periods = getUsagePricingPeriods(data, range);
  return periods
    ? { status: "ready", periods }
    : { status: "unavailable", reason: "unsupported-period" };
}

const DAY_MS = 24 * 60 * 60 * 1000;

export function getUsagePricingPeriods(
  prices: GetPricesResponse | undefined,
  range: DateRange | null,
): UsagePricingPeriod[] | undefined {
  if (!prices || !range) return undefined;
  const start = Date.parse(range.from);
  // Pricing timestamps use half-open intervals; include the full final usage day.
  const end = Date.parse(range.to) + DAY_MS;
  if (!Number.isFinite(start) || !Number.isFinite(end) || start >= end) {
    return undefined;
  }

  const slices: UsagePricingPeriod[] = [];
  let coveredUntil = start;
  for (const period of [...prices.periods].sort((a, b) => a.start - b.start)) {
    const sliceStart = Math.max(start, period.start);
    const sliceEnd = Math.min(end, period.end ?? end);
    if (sliceStart >= sliceEnd) continue;
    // Date-only usage queries cannot attribute an intraday price change exactly.
    if (
      !Number.isFinite(sliceStart) ||
      !Number.isFinite(sliceEnd) ||
      sliceStart !== coveredUntil ||
      (slices.length > 0 &&
        period.plan.planType !== slices[0].period.plan.planType) ||
      sliceStart % DAY_MS !== 0 ||
      sliceEnd % DAY_MS !== 0
    ) {
      return undefined;
    }
    slices.push({
      period,
      range: {
        from: new Date(sliceStart).toISOString().slice(0, 10),
        to: new Date(sliceEnd - DAY_MS).toISOString().slice(0, 10),
      },
    });
    coveredUntil = sliceEnd;
  }
  return coveredUntil === end && slices.length > 0 ? slices : undefined;
}
