"use client";

import * as React from "react";
import { ChevronLeftIcon, ChevronRightIcon } from "@radix-ui/react-icons";
import {
  ClassNames,
  DayButton,
  DayButtonProps,
  DayPicker,
  DayProps,
  NextMonthButtonProps,
  PreviousMonthButtonProps,
} from "react-day-picker";

import { cn } from "@ui/cn";
import { Tooltip } from "@ui/Tooltip";

export type BaseProps = React.ComponentProps<typeof DayPicker>;
export type CalendarProps = BaseProps & {
  beforeStartTooltip?: React.ReactNode;
};

export function Calendar({
  beforeStartTooltip,
  ...calendarProps
}: CalendarProps) {
  return (
    <DayPicker
      classNames={
        {
          root: "[--calendar-header-height:--spacing(7)] relative",
          months: "flex flex-col sm:flex-row gap-y-4 sm:gap-x-4 sm:gap-y-0",
          month: "flex flex-col gap-y-3",
          month_caption:
            "flex justify-center items-center min-h-(--calendar-header-height)",
          caption_label: "text-sm font-medium",
          nav: "absolute inset-x-0 top-0 flex items-center min-h-(--calendar-header-height) justify-between px-1 pointer-events-none",
          button_previous: cn(
            "pointer-events-auto flex size-(--calendar-header-height) items-center justify-center bg-transparent p-0 opacity-50 hover:opacity-100 aria-disabled:cursor-not-allowed aria-disabled:hover:opacity-50",
          ),
          button_next: cn(
            "pointer-events-auto flex size-(--calendar-header-height) items-center justify-center bg-transparent p-0 opacity-50 hover:opacity-100 aria-disabled:cursor-not-allowed aria-disabled:hover:opacity-50",
          ),
          month_grid: "w-full border-collapse space-y-1",
          weekdays: "flex",
          weekday: "rounded-md w-8 font-normal text-[0.8rem]",
          week: "flex w-fit mt-2 first:ml-auto",
          day: cn(
            "size-8 p-0 text-center text-sm focus-within:relative focus-within:z-20",
          ),
          day_button: cn(
            "size-full p-0 aria-selected:opacity-100",
            "hover:rounded-sm hover:bg-background-primary",
          ),
          selected: "bg-background-tertiary border-y",
          range_start: "rounded-l bg-background-tertiary border",
          range_end: "rounded-r bg-background-tertiary border",
          range_middle:
            "bg-background-tertiary/30 border-y hover:text-primary-foreground focus:bg-primary",
          today: "font-semibold",
          outside: "day-outside opacity-30 aria-selected:opacity-50",
          disabled:
            "opacity-60 [&>button]:cursor-not-allowed [&>button]:hover:bg-transparent",
          hidden: "invisible",
        } satisfies Partial<ClassNames>
      }
      components={{
        PreviousMonthButton: function CustomPreviousMonthButton({
          children: _,
          ...buttonProps
        }: PreviousMonthButtonProps) {
          return (
            <Tooltip
              tip={buttonProps["aria-disabled"] === true && beforeStartTooltip}
              asChild
            >
              {/* eslint-disable-next-line react/forbid-elements -- Component managed by react-day-picker */}
              <button {...buttonProps}>
                <ChevronLeftIcon className="size-4" />
              </button>
            </Tooltip>
          );
        },

        NextMonthButton: function CustomPreviousMonthButton({
          children: _,
          ...buttonProps
        }: NextMonthButtonProps) {
          return (
            // eslint-disable-next-line react/forbid-elements -- Component managed by react-day-picker
            <button {...buttonProps}>
              <ChevronRightIcon className="size-4" />
            </button>
          );
        },

        // Modify `DayButton` to forward ref so that we can wrap it in a tooltip
        DayButton: React.forwardRef<HTMLButtonElement, DayButtonProps>(
          function CalendarDayButton(
            { day: _, modifiers, ...buttonProps },
            ref,
          ) {
            const localRef = React.useRef<HTMLButtonElement>(null);
            React.useImperativeHandle(ref, () => localRef.current!);

            React.useEffect(() => {
              if (modifiers.focused) localRef.current?.focus();
            }, [modifiers.focused]);

            // eslint-disable-next-line react/forbid-elements -- Component managed by react-day-picker
            return <button ref={localRef} {...buttonProps} />;
          },
        ) as typeof DayButton, // need `as` here since `DayButton` isn’t

        // Modify `Day` to wrap the children in a tooltip when necessary
        Day: function CalendarDay({
          day,
          modifiers,
          children,
          ...tdProps
        }: DayProps) {
          return (
            <td {...tdProps}>
              <Tooltip
                tip={
                  beforeStartTooltip &&
                  modifiers.disabled &&
                  typeof calendarProps.disabled === "object" &&
                  "before" in calendarProps.disabled &&
                  day.date < calendarProps.disabled.before
                    ? beforeStartTooltip
                    : undefined
                }
                asChild
              >
                {children}
              </Tooltip>
            </td>
          );
        },
      }}
      {...calendarProps}
    />
  );
}
