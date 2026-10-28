import {
  CrossCircledIcon,
  ExclamationTriangleIcon,
} from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import React from "react";
import { Sheet } from "@ui/Sheet";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { LoadingTransition } from "@ui/Loading";

export function HealthCard({
  title,
  tip,
  children,
  className,
  error,
  warning,
  size = "md",
  action,
}: {
  title: string;
  children?: React.ReactNode;
  className?: string;
  error?: React.ReactNode;
  warning?: React.ReactNode;
  size?: "xs" | "sm" | "md" | "lg";
  action?: React.ReactNode;
  tip?: React.ReactNode;
}) {
  return (
    <Sheet
      padding={false}
      className={cn(
        "flex w-full min-w-64 animate-fadeInFromLoading flex-col transition-all",
        size === "xs" && "h-fit",
        size === "sm" && "min-h-fit",
        size === "md" && "min-h-56",
        size === "lg" && "max-h-84 min-h-fit w-full",
      )}
    >
      <div className="relative flex grow flex-col transition-all">
        <div className="flex items-center justify-between gap-2 p-2">
          <h5 className="truncate">{title}</h5>
          <div className="flex items-center gap-1">
            {warning && (
              <Tooltip
                className="flex gap-1 rounded-sm border bg-background-warning p-0.5 text-xs text-content-warning"
                tip={<div>{warning}</div>}
              >
                <ExclamationTriangleIcon />
              </Tooltip>
            )}
            {error && (
              <Tooltip
                className="flex gap-1 rounded-sm border bg-background-error p-0.5 text-xs text-content-error"
                tip={<div>{error}</div>}
              >
                <CrossCircledIcon />
              </Tooltip>
            )}
            {action}
            {tip && <HelpTooltip>{tip}</HelpTooltip>}
          </div>
        </div>
        {size !== "xs" && (
          <LoadingTransition>
            {children && (
              <div
                className={cn(
                  "flex grow flex-col items-center justify-center",
                  className,
                )}
              >
                {children}
              </div>
            )}
          </LoadingTransition>
        )}
      </div>
    </Sheet>
  );
}
