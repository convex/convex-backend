import type {
  CurrentPlanPriceResponse as MetricPrice,
  GetPricesPeriodResponse as PricesPeriod,
  MetricPriceAmount as PriceAmount,
} from "generatedApi";
import type { UnitPrice } from "@common/lib/deploymentContext";
import type { UsageSummaryRow } from "hooks/usageMetrics";

export type PricedUsageState =
  | { status: "loading" }
  | { status: "error"; error: unknown }
  | { status: "ready"; summaries: PricedUsageSummary[] };

type UsageField = keyof Omit<UsageSummaryRow, "deploymentClass" | "region">;
type Metric = {
  name: string;
  field: UsageField | UsageField[] | ((row: UsageSummaryRow) => number);
  divisor?: number;
};
type MetricSection = Metric | Metric[];

const BYTES_PER_GB = 2 ** 30;

const sections: Record<string, MetricSection> = {
  compute: [
    {
      name: "queryMutationComputeGbHours",
      field: "queryMutationCompute",
    },
    { name: "actionComputeCpuGbHours", field: "actionComputeUser" },
    { name: "actionComputeNodeJsGbHours", field: "actionComputeNode" },
  ],
  actionCompute: {
    name: "actionComputeConvexGbHours",
    field: ["actionComputeConvex", "actionComputeNode"],
  },
  functionCalls: { name: "functionCalls", field: "functionCalls" },
  // No divisor for search queries as the query already returns GB.
  searchQueries: { name: "searchQueryGb", field: "searchQueries" },
  databaseStorage: {
    name: "databaseStorageGb",
    field: "databaseStorage",
    divisor: BYTES_PER_GB,
  },
  databaseIO: {
    name: "databaseIoGb",
    field: "databaseIO",
    divisor: BYTES_PER_GB,
  },
  fileStorage: {
    name: "fileStorageGb",
    field: "fileStorage",
    divisor: BYTES_PER_GB,
  },
  searchStorage: {
    name: "searchStorageGb",
    field: "searchStorage",
    divisor: BYTES_PER_GB,
  },
  dataEgress: {
    name: "dataEgressGb",
    field: "dataEgress",
    divisor: BYTES_PER_GB,
  },
  auditLogBandwidth: {
    name: "auditLogBandwidthGb",
    field: "auditLogBandwidth",
    divisor: BYTES_PER_GB,
  },
  // The deployment fee is waived for paused and idle deployments.
  deploymentCount: {
    name: "activeDeployments",
    field: (row) =>
      Math.max(
        row.deploymentCount -
          row.pausedDeploymentCount -
          row.idleDeploymentCount,
        0,
      ),
  },
};

function parseNonNegativeNumber(value: string): number | undefined {
  if (value.trim() === "") return undefined;
  const number = Number(value);
  return Number.isFinite(number) && number >= 0 ? number : undefined;
}

function discountedRate(price: PriceAmount | undefined): number | undefined {
  if (!price) return undefined;
  const amount = parseNonNegativeNumber(price.amount);
  const discount = price.percentageDiscount ?? 0;
  if (
    amount === undefined ||
    !Number.isFinite(discount) ||
    discount < 0 ||
    discount > 1
  ) {
    return undefined;
  }
  return amount * (1 - discount);
}

// The rate that applies to usage in a deployment class and region, if the
// price has one.
function priceAmountFor(
  price: MetricPrice,
  { deploymentClass, region }: { deploymentClass: string; region: string },
): PriceAmount | undefined {
  return "rate" in price
    ? price.rate
    : "regionRate" in price
      ? price.regionRate[region]
      : price.deploymentClassRate[deploymentClass]?.regionRate[region];
}

// The undiscounted price of one unit of each metric for a deployment in a
// pricing period, and whether a discount applies to it, keyed by metric name.
// Metrics without a valid rate for the deployment's class and region are
// omitted.
export function getDeploymentUnitPrices(
  period: PricesPeriod,
  deployment: { deploymentClass: string; region: string },
): Record<string, UnitPrice> {
  const unitPrices: Record<string, UnitPrice> = {};
  for (const [metric, price] of Object.entries(period.metrics)) {
    const amount = priceAmountFor(price, deployment);
    const listPrice = amount && parseNonNegativeNumber(amount.amount);
    if (!amount || listPrice === undefined) {
      continue;
    }
    unitPrices[metric] = {
      listPrice,
      hasDiscount: (amount.percentageDiscount ?? 0) > 0,
    };
  }
  return unitPrices;
}

export type UsageSpend = {
  amount: number;
  includedAmount: number;
  missingDimensions: string[];
};

function priceAmounts(price: MetricPrice | undefined): PriceAmount[] {
  return !price
    ? []
    : "rate" in price
      ? [price.rate]
      : "regionRate" in price
        ? Object.values(price.regionRate)
        : Object.values(price.deploymentClassRate).flatMap((regional) =>
            Object.values(regional.regionRate),
          );
}

export function getIncludedUsage(
  periods: {
    period: PricesPeriod;
    summary?: UsageSummaryRow[];
    teamSummary?: UsageSummaryRow[];
  }[],
  sectionMetric: string,
): number | undefined {
  if (!Object.hasOwn(sections, sectionMetric)) return undefined;
  const section = sections[sectionMetric];
  const metrics = Array.isArray(section) ? section : [section];
  let total = 0;
  let hasConsumedUnits = false;
  for (const { period, summary, teamSummary } of periods) {
    for (const metric of metrics) {
      const price = period.metrics[metric.name];
      const shares =
        summary && teamSummary
          ? rateShares(summary, teamSummary, metric, price)
          : { share: () => 1 };
      if (shares === undefined) return undefined;
      for (const rate of priceAmounts(price)) {
        const discount = rate.percentageDiscount ?? 0;
        if (!Number.isFinite(discount) || discount < 0 || discount > 1) {
          return undefined;
        }
        const consumed = rate.consumedUnits;
        if (consumed === undefined || consumed === null) continue;
        if (!Number.isFinite(consumed) || consumed < 0) return undefined;
        hasConsumedUnits = true;
        if (discount === 1) {
          if (consumed > 0) return undefined;
          continue;
        }
        // The consumed count is post-discount, so we need to un-apply the
        // discount to get the actual included usage.
        total +=
          (consumed / (1 - discount)) *
          (metric.divisor ?? 1) *
          shares.share(rate);
      }
    }
  }
  return hasConsumedUnits && Number.isFinite(total) ? total : undefined;
}

function usageByRate(
  summary: UsageSummaryRow[],
  metric: Metric,
  price: MetricPrice | undefined,
):
  | { usage: Map<PriceAmount, number>; missingDimensions: Set<string> }
  | undefined {
  const usage = new Map<PriceAmount, number>();
  const missingDimensions = new Set<string>();
  for (const row of summary) {
    const { field } = metric;
    const quantities = (
      typeof field === "function"
        ? [field(row)]
        : (Array.isArray(field) ? field : [field]).map((f) => row[f])
    ).map((value) => value / (metric.divisor ?? 1));
    if (
      quantities.some((quantity) => !Number.isFinite(quantity) || quantity < 0)
    ) {
      return undefined;
    }
    const quantity = quantities.reduce((sum, value) => sum + value, 0);
    if (quantity === 0) continue;
    if (!price) {
      missingDimensions.add(metric.name);
      continue;
    }
    const amount = priceAmountFor(price, row);
    if (!amount) {
      missingDimensions.add(
        "regionRate" in price
          ? row.region
          : `${row.deploymentClass} / ${row.region}`,
      );
      continue;
    }
    usage.set(amount, (usage.get(amount) ?? 0) + quantity);
  }
  return { usage, missingDimensions };
}

// The included allowance is shared by the whole team and the ledger records
// how much of it each rate consumed, but not which project's usage consumed it.
// Filtered usage gets the share of each rate's consumption that it makes up of
// the team's usage at that rate.
function rateShares(
  summary: UsageSummaryRow[],
  teamSummary: UsageSummaryRow[],
  metric: Metric,
  price: MetricPrice | undefined,
):
  | { share: (rate: PriceAmount) => number; missingDimensions: Set<string> }
  | undefined {
  const own = usageByRate(summary, metric, price);
  const team = usageByRate(teamSummary, metric, price);
  if (own === undefined || team === undefined) return undefined;
  return {
    share: (rate) => {
      const teamQuantity = team.usage.get(rate) ?? 0;
      // The two summaries come from separate queries, so cap the share in
      // case they were read at different times.
      return teamQuantity > 0
        ? Math.min((own.usage.get(rate) ?? 0) / teamQuantity, 1)
        : 0;
    },
    missingDimensions: own.missingDimensions,
  };
}

// Gauge metrics bill monthly on a snapshot rather than on accumulated usage, so
// their spend is a monthly estimate from the most recent snapshot.
export function isGaugeMetric(sectionMetric: string): boolean {
  return (
    sectionMetric === "databaseStorage" ||
    sectionMetric === "fileStorage" ||
    sectionMetric === "searchStorage" ||
    sectionMetric === "deploymentCount"
  );
}

// With `share`, each rate's spend and included amount are scaled by it.
function metricSpend(
  summary: UsageSummaryRow[],
  metric: Metric,
  price: MetricPrice | undefined,
  estimateMonthly: boolean,
  share: (rate: PriceAmount) => number = () => 1,
): UsageSpend | undefined {
  let gross = 0;
  let estimatedUnits = 0;
  let coveredSpend = 0;

  const rates = estimateMonthly ? [] : priceAmounts(price);
  for (const amount of rates) {
    if (amount.consumedUnits === undefined || amount.consumedUnits === null) {
      continue;
    }
    const rate = discountedRate(amount);

    if (
      rate === undefined ||
      !Number.isFinite(amount.consumedUnits) ||
      amount.consumedUnits < 0
    ) {
      return undefined;
    }
    // consumedUnits is already post-discount application, so multiply by the
    // undiscounted rate here.
    coveredSpend +=
      amount.consumedUnits * Number(amount.amount) * share(amount);
  }
  const byRate = usageByRate(summary, metric, price);
  if (byRate === undefined) return undefined;
  const { usage, missingDimensions } = byRate;
  for (const [amount, quantity] of usage) {
    const rate = discountedRate(amount);
    if (rate === undefined) return undefined;
    const roundedQuantity = Math.ceil(quantity);
    gross += roundedQuantity * rate * share(amount);
    // The allowance applies to the team's total, so units aren't scaled.
    estimatedUnits += roundedQuantity;
  }
  if (estimateMonthly) {
    const includedUnits = price
      ? parseNonNegativeNumber(price.includedUnits)
      : 0;
    if (includedUnits === undefined) return undefined;
    // Monthly estimates value a shared allowance at the snapshot's weighted
    // average rate across deployment classes and regions.
    coveredSpend =
      estimatedUnits > 0
        ? gross * Math.min(includedUnits / estimatedUnits, 1)
        : 0;
  }
  return Number.isFinite(gross) && Number.isFinite(coveredSpend)
    ? {
        amount: gross,
        includedAmount: coveredSpend,
        missingDimensions: [...missingDimensions].sort(),
      }
    : undefined;
}

// Filtered usage can't be priced on its own because the included allowance is
// shared by the whole team. Its spend is the team's spend with each rate scaled
// by the filtered usage's part of the team's usage at that rate, so each rate's
// rounding and included consumption stay with the usage that incurred them.
function metricSpendShare(
  summary: UsageSummaryRow[],
  teamSummary: UsageSummaryRow[],
  metric: Metric,
  price: MetricPrice | undefined,
  estimateMonthly: boolean,
): UsageSpend | undefined {
  const shares = rateShares(summary, teamSummary, metric, price);
  if (shares === undefined) return undefined;
  const teamSpend = metricSpend(
    teamSummary,
    metric,
    price,
    estimateMonthly,
    shares.share,
  );
  if (teamSpend === undefined) return undefined;
  return {
    amount: teamSpend.amount,
    includedAmount: teamSpend.includedAmount,
    missingDimensions: mergeMissingDimensions(
      { missingDimensions: [...shares.missingDimensions] },
      teamSpend,
    ),
  };
}

// Usage the team can't price makes the team's spend, and so the share,
// incomplete even when the filtered usage has none of it.
function mergeMissingDimensions(
  ...spends: Pick<UsageSpend, "missingDimensions">[]
): string[] {
  return [
    ...new Set(spends.flatMap(({ missingDimensions }) => missingDimensions)),
  ].sort();
}

// Whether any metric in the periods draws on an included allowance. Without
// one, filtered usage is priced directly and needs no team-wide usage.
export function hasIncludedUsage(periods: PricesPeriod[]): boolean {
  return periods.some((period) =>
    Object.values(period.metrics).some(
      (price) =>
        parseNonNegativeNumber(price.includedUnits) !== 0 ||
        priceAmounts(price).some(
          ({ consumedUnits }) =>
            consumedUnits !== undefined &&
            consumedUnits !== null &&
            consumedUnits !== 0,
        ),
    ),
  );
}

// With `teamSummary`, `summary` is filtered usage and its spend is a share of
// the team's spend.
export function getUsageSpend(
  summary: UsageSummaryRow[] | undefined,
  sectionMetric: string,
  pricesPeriod: PricesPeriod | undefined,
  teamSummary?: UsageSummaryRow[],
): UsageSpend | undefined {
  if (!summary || !pricesPeriod) return undefined;
  if (!Object.hasOwn(sections, sectionMetric)) {
    return undefined;
  }

  const section = sections[sectionMetric];
  const metrics = Array.isArray(section) ? section : [section];
  if (
    !metrics.some((metric) => pricesPeriod.metrics[metric.name] !== undefined)
  ) {
    return undefined;
  }

  let total = 0;
  let includedAmount = 0;
  const missingDimensions = new Set<string>();
  for (const metric of metrics) {
    const price = pricesPeriod.metrics[metric.name];
    const estimateMonthly = isGaugeMetric(sectionMetric);
    const spend = teamSummary
      ? metricSpendShare(summary, teamSummary, metric, price, estimateMonthly)
      : metricSpend(summary, metric, price, estimateMonthly);
    if (spend === undefined) return undefined;
    total += spend.amount;
    includedAmount += spend.includedAmount;
    for (const dimension of spend.missingDimensions) {
      missingDimensions.add(dimension);
    }
  }
  return Number.isFinite(total)
    ? {
        amount: total,
        includedAmount,
        missingDimensions: [...missingDimensions].sort(),
      }
    : undefined;
}

export type PricedUsageSummary = {
  period: PricesPeriod;
  summary: UsageSummaryRow[];
  // Team-wide usage for the period, present when `summary` is filtered and
  // the team has included usage to share out.
  teamSummary?: UsageSummaryRow[];
};

export function getMultiPeriodUsageSpend(
  pricedSummaries: PricedUsageSummary[] | undefined,
  sectionMetric: string,
): UsageSpend | undefined {
  if (!pricedSummaries?.length) return undefined;

  let total = 0;
  let includedAmount = 0;
  const missingDimensions = new Set<string>();
  for (const { period, summary, teamSummary } of pricedSummaries) {
    const spend = getUsageSpend(summary, sectionMetric, period, teamSummary);
    if (spend === undefined) return undefined;
    total += spend.amount;
    includedAmount += spend.includedAmount;
    for (const dimension of spend.missingDimensions) {
      missingDimensions.add(dimension);
    }
  }
  return Number.isFinite(total)
    ? {
        amount: total,
        includedAmount,
        missingDimensions: [...missingDimensions].sort(),
      }
    : undefined;
}

export function formatSpend(value: number): string {
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 2,
    maximumFractionDigits: 2,
  }).format(value);
}
