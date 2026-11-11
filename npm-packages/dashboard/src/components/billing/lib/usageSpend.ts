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
  periods: PricesPeriod[],
  sectionMetric: string,
): number | undefined {
  if (!Object.hasOwn(sections, sectionMetric)) return undefined;
  const section = sections[sectionMetric];
  const metrics = Array.isArray(section) ? section : [section];
  let total = 0;
  let hasConsumedUnits = false;
  for (const period of periods) {
    for (const metric of metrics) {
      for (const rate of priceAmounts(period.metrics[metric.name])) {
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
        total += (consumed / (1 - discount)) * (metric.divisor ?? 1);
      }
    }
  }
  return hasConsumedUnits && Number.isFinite(total) ? total : undefined;
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

function metricSpend(
  summary: UsageSummaryRow[],
  metric: Metric,
  price: MetricPrice | undefined,
  estimateMonthly: boolean,
): UsageSpend | undefined {
  let gross = 0;
  let estimatedUnits = 0;
  let coveredSpend = 0;
  const missingDimensions = new Set<string>();

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
    coveredSpend += amount.consumedUnits * Number(amount.amount);
  }
  const usageByDimension = new Map<
    string,
    { quantity: number; amount: PriceAmount }
  >();
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
    const dimension =
      "rate" in price
        ? "global"
        : "regionRate" in price
          ? row.region
          : JSON.stringify([row.deploymentClass, row.region]);
    const existing = usageByDimension.get(dimension);
    usageByDimension.set(dimension, {
      quantity: (existing?.quantity ?? 0) + quantity,
      amount,
    });
  }
  for (const { quantity, amount } of usageByDimension.values()) {
    const rate = discountedRate(amount);
    if (rate === undefined) return undefined;
    const roundedQuantity = Math.ceil(quantity);
    gross += roundedQuantity * rate;
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

export function getUsageSpend(
  summary: UsageSummaryRow[] | undefined,
  sectionMetric: string,
  pricesPeriod: PricesPeriod | undefined,
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
    const spend = metricSpend(
      summary,
      metric,
      pricesPeriod.metrics[metric.name],
      isGaugeMetric(sectionMetric),
    );
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
};

export function getMultiPeriodUsageSpend(
  pricedSummaries: PricedUsageSummary[] | undefined,
  sectionMetric: string,
): UsageSpend | undefined {
  if (!pricedSummaries?.length) return undefined;

  let total = 0;
  let includedAmount = 0;
  const missingDimensions = new Set<string>();
  for (const { period, summary } of pricedSummaries) {
    const spend = getUsageSpend(summary, sectionMetric, period);
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
