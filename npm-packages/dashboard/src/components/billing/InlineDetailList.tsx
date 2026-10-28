import { Tooltip } from "@ui/Tooltip";
import { QuantityType, formatQuantity } from "./lib/formatQuantity";

export interface InlineDetailItem {
  name: string;
  value: number;
  /** Used for sort order when provided (e.g. total-range value). Falls back to value. */
  sortValue?: number;
  color: string;
  /** When set, the label gets a dotted underline and this tooltip on hover. */
  tip?: string;
}

export function InlineDetailList({
  items,
  quantityType,
  showZeroValues = false,
}: {
  items: InlineDetailItem[];
  quantityType: QuantityType;
  /**
   * Keep entries whose value is zero. Off by default so charts don't list
   * categories a team never uses; on for fixed category sets (e.g. deployment
   * status) where every category should always appear in the legend.
   */
  showZeroValues?: boolean;
}) {
  const total = items.reduce((sum, item) => sum + item.value, 0);
  // sortValue only governs ordering (kept stable across day selection); an item
  // is visible when either its shown value or its sort value is nonzero, so a
  // category present on the selected day still shows even if its range/gauge
  // total is zero.
  const sortedItems = items
    .filter(
      (item) => showZeroValues || item.value > 0 || (item.sortValue ?? 0) > 0,
    )
    .sort((a, b) => (b.sortValue ?? b.value) - (a.sortValue ?? a.value));

  return (
    <div className="flex flex-col gap-1">
      {sortedItems.map((item, index) => {
        const percentage = total > 0 ? (item.value / total) * 100 : 0;
        return (
          <div
            key={index}
            className="flex items-center justify-between gap-4 rounded-sm px-2 py-1 text-sm hover:bg-slate-900/5 dark:hover:bg-white/5"
          >
            <span className="flex items-center gap-2">
              <span
                className="size-2.5 shrink-0 rounded-full"
                style={{
                  backgroundColor: `var(--color-${item.color.replace("fill-", "")})`,
                }}
              />
              {item.tip ? (
                <Tooltip
                  tip={item.tip}
                  className="truncate underline decoration-dotted"
                >
                  {item.name}
                </Tooltip>
              ) : (
                <span className="truncate">{item.name}</span>
              )}
            </span>
            <span className="flex shrink-0 items-center gap-3 tabular-nums">
              <span>{formatQuantity(item.value, quantityType)}</span>
              {sortedItems.length > 1 && (
                <span className="w-12 text-right opacity-70">
                  {percentage.toFixed(1)}%
                </span>
              )}
            </span>
          </div>
        );
      })}
    </div>
  );
}
