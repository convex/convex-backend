import { formatBytes, formatNumberCompact } from "@common/lib/format";

export type QuantityType =
  // Receives data in units of the given entity name
  | "unit"
  // Receives data in bytes, displays them using the appropriate data size unit
  | "storage"
  // Receives and displays data in GB * hours
  | "actionCompute"
  // Receives and displays data in query-GB
  | "textSearch"
  // Receives and displays data in US dollars
  | "currency";

const ACTION_COMPUTE_FORMAT_DECIMAL = new Intl.NumberFormat("en-US", {
  minimumFractionDigits: 0,
  maximumFractionDigits: 5,
});

const CURRENCY_FORMAT = new Intl.NumberFormat("en-US", {
  style: "currency",
  currency: "USD",
  minimumFractionDigits: 2,
  maximumFractionDigits: 4,
});

function formatActionCompute(value: number): string {
  // Show decimals for values less than 1, compact formatting for larger values
  if (value < 1) {
    return ACTION_COMPUTE_FORMAT_DECIMAL.format(value);
  }
  return formatNumberCompact(value);
}

export function formatQuantity(value: number, quantityType: QuantityType) {
  switch (quantityType) {
    case "storage":
      return formatBytes(value);
    case "actionCompute":
      return `${formatActionCompute(value)} GB-hours`;
    case "textSearch":
      return `${formatActionCompute(value)} Query-GB`;
    case "currency":
      return CURRENCY_FORMAT.format(value);
    default:
      return formatNumberCompact(value);
  }
}

export function formatQuantityCompact(
  value: number,
  quantityType: QuantityType,
) {
  switch (quantityType) {
    case "storage":
      return formatBytes(value);
    case "actionCompute":
      // non-breaking space to keep this on a single line
      return `${formatActionCompute(value)}\u00a0GBh`;
    case "textSearch":
      return `${formatActionCompute(value)}\u00a0qGB`;
    case "currency":
      return CURRENCY_FORMAT.format(value);
    default:
      return formatNumberCompact(value);
  }
}
