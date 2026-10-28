import { DateRangePicker } from "@common/elements/DateRangePicker";
import { toDateInputValue } from "@common/lib/format";
import { subDays, subMonths } from "date-fns";

/**
 * The date at which valid usage data starts. Users can’t see the data before.
 */
const USAGE_STATS_START = "2023-08-03";

export type Period = {
  type: "currentBillingPeriod" | "presetPeriod" | "customPeriod";
  from: string; // ISO 8601 date strings
  to: string;
};

/**
 * @param dateString e.g. "2023-11-02"
 * @returns e.g. "2023-10-02"
 */
function monthBefore(dateString: string): string {
  const [year, month, day] = dateString.split("-");
  const date = new Date(Number(year), Number(month) - 1, Number(day));
  date.setMonth(date.getMonth() - 1);
  return toDateInputValue(date);
}

function startOfDayUTC(date: string): Date {
  return new Date(new Date(date).toLocaleString("en-US", { timeZone: "UTC" }));
}

export function UsagePeriodSelector({
  period,
  onChange,
  currentBillingPeriod,
}: {
  period: Period;
  onChange: (newValue: Period) => void;
  currentBillingPeriod: { start: string; end: string };
}) {
  const from = startOfDayUTC(period.from);
  const to = startOfDayUTC(period.to);
  const currentBillingPeriodStart = startOfDayUTC(currentBillingPeriod.start);
  const currentBillingPeriodEnd = startOfDayUTC(currentBillingPeriod.end);
  const lastBillingPeriodStart = startOfDayUTC(
    monthBefore(currentBillingPeriod.start),
  );
  const lastBillingPeriodEnd = startOfDayUTC(currentBillingPeriod.start);

  const today = startOfDayUTC(toDateInputValue(new Date()));
  const weekAgo = startOfDayUTC(toDateInputValue(subDays(today, 7)));
  const monthAgo = startOfDayUTC(toDateInputValue(subDays(today, 30)));
  const quarterAgo = startOfDayUTC(toDateInputValue(subMonths(today, 3)));

  return (
    <DateRangePicker
      minDate={new Date(USAGE_STATS_START)}
      shortcuts={[
        {
          value: "current",
          label: "Current billing period",
          from: currentBillingPeriodStart,
          to: currentBillingPeriodEnd,
        },
        {
          value: "last",
          label: "Last billing period",
          from: lastBillingPeriodStart,
          to: lastBillingPeriodEnd,
        },
        {
          value: "week",
          label: "Last 7 days",
          from: weekAgo,
          to: today,
        },
        {
          value: "month",
          label: "Last 30 days",
          from: monthAgo,
          to: today,
        },
        {
          value: "quarter",
          label: "Last 3 months",
          from: quarterAgo,
          to: today,
        },
      ]}
      date={{
        from,
        to,
      }}
      setDate={(date, shortcut) => {
        if (date.from && date.to) {
          onChange(
            shortcut?.value === "current"
              ? {
                  type: "currentBillingPeriod",
                  from: currentBillingPeriod.start,
                  to: currentBillingPeriod.end,
                }
              : shortcut?.value === "last"
                ? {
                    type: "presetPeriod",
                    from: monthBefore(currentBillingPeriod.start),
                    to: currentBillingPeriod.start,
                  }
                : {
                    type: "customPeriod",
                    from: toDateInputValue(date.from),
                    to: toDateInputValue(date.to),
                  },
          );
        }
      }}
    />
  );
}
