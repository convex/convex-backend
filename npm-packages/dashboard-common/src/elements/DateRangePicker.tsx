"use client";

import { CalendarIcon, CheckIcon } from "@radix-ui/react-icons";
import { endOfToday, parse, startOfDay, format } from "date-fns";
import { NextRouter } from "next/router";
import * as React from "react";
import { DateRange, Matcher } from "react-day-picker";
import { Popover } from "@ui/Popover";
import { Button } from "@ui/Button";
import { Calendar } from "@common/elements/Calendar";

export type DateRangeShortcut = {
  value: string;
  label: string;
  from: Date;
  to: Date;
  disableFilters?: boolean;
};

export function DateRangePicker({
  minDate,
  maxDate,
  beforeMinDateTooltip,
  date,
  setDate,
  shortcuts,
  formatDate = (d: Date) => format(d, "LLL dd, y"),
  disabled = false,
  dateFilterEnabled = true,
  prefix,
}: {
  minDate?: Date;
  maxDate?: Date;
  beforeMinDateTooltip?: React.ReactNode;
  date: {
    from?: Date;
    to?: Date;
  };
  setDate: (date: DateRange, shortcut?: DateRangeShortcut) => void;
  shortcuts?: DateRangeShortcut[];
  formatDate?: (date: Date) => string;
  disabled?: boolean;
  dateFilterEnabled?: boolean;
  prefix?: string;
}) {
  const { from, to } = date;

  // Track current selected shortcut
  const [activeShortcut, setActiveShortcut] = React.useState<string | null>(
    null,
  );

  // Initialize activeShortcut based on dateFilterEnabled
  React.useEffect(() => {
    if (!dateFilterEnabled) {
      // Set to "anytime" when filters are disabled
      setActiveShortcut("anytime");
    } else if (shortcuts && from && to) {
      // Check if current date range matches any shortcut
      const matchingShortcut = shortcuts.find(
        (s) =>
          !s.disableFilters &&
          s.from.toDateString() === from.toDateString() &&
          s.to.toDateString() === to.toDateString(),
      );
      setActiveShortcut(matchingShortcut?.value || null);
    }
  }, [dateFilterEnabled, from, to, shortcuts]);

  // When a calendar selection is made, we need to track it for rendering,
  // but defer to the parent component for state management
  const [selectedRange, setSelectedRange] = React.useState<
    DateRange | undefined
  >(from && to ? { from, to } : undefined);

  // Update selected range when date props change
  React.useEffect(() => {
    setSelectedRange(from && to ? { from, to } : undefined);
  }, [from, to]);

  return (
    <Popover
      placement="bottom-start"
      button={
        <Button
          variant="neutral"
          className="w-fit justify-start text-left font-normal"
          icon={<CalendarIcon className="size-4" />}
          disabled={disabled}
        >
          <div className="flex items-center gap-1">
            {prefix && <span className="font-semibold">{prefix}</span>}
            {dateFilterEnabled ? (
              from && to ? (
                <>
                  {formatDate(from)} – {formatDate(to)}
                </>
              ) : from ? (
                formatDate(from)
              ) : (
                <span>Pick a date</span>
              )
            ) : (
              <span>Any time</span>
            )}
          </div>
        </Button>
      }
    >
      {({ close }) => (
        <div className="flex flex-col gap-4 md:flex-row">
          {shortcuts && (
            <div className="flex w-52 flex-col gap-2 border-b pb-4 md:border-r md:border-b-0 md:pb-0">
              {shortcuts.map((s) => (
                <Button
                  key={s.label}
                  variant="unstyled"
                  onClick={() => {
                    setActiveShortcut(s.value);
                    // Clear the internal selected range for "Any time"
                    if (s.disableFilters) {
                      setSelectedRange(undefined);
                    } else {
                      setSelectedRange({ from: s.from, to: s.to });
                    }
                    close();
                    setDate(s, s);
                  }}
                  className="-ml-4 flex w-full items-start gap-1 rounded-sm p-1 text-xs hover:bg-background-tertiary"
                  icon={
                    activeShortcut === s.value ? (
                      <CheckIcon className="mt-1" />
                    ) : (
                      <div className="size-4" />
                    )
                  }
                >
                  <div className="flex flex-col items-start">
                    {s.label}{" "}
                    {!s.disableFilters ? (
                      <span className="text-xs text-content-secondary">
                        {formatDate(s.from)} – {formatDate(s.to)}
                      </span>
                    ) : (
                      <span className="text-xs text-content-secondary">
                        Show all results
                      </span>
                    )}
                  </div>
                </Button>
              ))}
            </div>
          )}
          <Calendar
            autoFocus
            mode="range"
            startMonth={minDate}
            endMonth={maxDate}
            disabled={disabledFromRange({ minDate, maxDate })}
            beforeStartTooltip={beforeMinDateTooltip}
            defaultMonth={from || new Date()}
            selected={selectedRange}
            onSelect={(d) => {
              if (!d) return;

              // Clear active shortcut when manually selecting dates
              setActiveShortcut(null);

              // Update internal selected range for immediate UI feedback
              setSelectedRange(d);

              // Pass the date range to parent component
              setDate(d);
            }}
            numberOfMonths={2}
          />
        </div>
      )}
    </Popover>
  );
}

export const DATE_FORMAT = "yyyy-MM-dd";

export function useDateFilters(router: NextRouter) {
  // Current day
  const maxEndDate = endOfToday();

  // A week from current day
  const initStartDate = new Date(maxEndDate);
  initStartDate.setDate(initStartDate.getDate() - 7);

  const startDate = router.query.startDate
    ? parse(router.query.startDate as string, DATE_FORMAT, new Date())
    : initStartDate;
  const endDate = router.query.endDate
    ? parse(router.query.endDate as string, DATE_FORMAT, new Date())
    : maxEndDate;

  const checkAndSetStartDate = React.useCallback(
    async (date: Date) => {
      const start = startOfDay(date);
      router.query.startDate = format(start, DATE_FORMAT);
      await router.replace({
        query: router.query,
      });
    },
    [router],
  );

  const checkAndSetEndDate = React.useCallback(
    async (date: Date) => {
      const end = startOfDay(date);
      router.query.endDate = format(end, DATE_FORMAT);
      await router.replace({
        query: router.query,
      });
    },
    [router],
  );
  return {
    startDate,
    endDate,
    setDate: async (date: DateRange) => {
      if (date.from) {
        await checkAndSetStartDate(date.from);
      }
      if (date.to) {
        await checkAndSetEndDate(date.to);
      }
    },
  };
}

function disabledFromRange({
  minDate,
  maxDate,
}: {
  minDate: Date | undefined;
  maxDate: Date | undefined;
}): Matcher | undefined {
  if (minDate && maxDate) {
    return { before: minDate, after: maxDate };
  }
  if (minDate) {
    return { before: minDate };
  }
  if (maxDate) {
    return { after: maxDate };
  }
  return undefined;
}
