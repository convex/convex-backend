import React from "react";
import * as RadixTooltip from "@radix-ui/react-tooltip";
import classNames from "classnames";
import { usePortalContainer } from "./PortalContainer";

export type TooltipSide = "left" | "right" | "bottom" | "top";

export function Tooltip({
  children,
  tip,
  side = "bottom",
  align = "center",
  className,
  contentClassName,
  asChild = false,
  delayDuration = 0,
  maxWidthClassName = "max-w-[16rem]",
  disableHoverableContent = false,
  ["aria-label"]: ariaLabel,
}: {
  children: React.ReactNode;
  tip: React.ReactNode | undefined;
  side?: TooltipSide;
  align?: "start" | "end" | "center";
  className?: string;
  contentClassName?: string;
  maxWidthClassName?: string;
  asChild?: boolean;
  delayDuration?: number;
  disableHoverableContent?: boolean;
  "aria-label"?: string;
}) {
  const portalContainer = usePortalContainer();
  // Some existing callsites pass in boolean so we do a truthy check
  if (!tip) {
    return <>{children}</>;
  }
  return (
    <RadixTooltip.Provider
      delayDuration={delayDuration}
      disableHoverableContent={disableHoverableContent}
    >
      <RadixTooltip.Root>
        <RadixTooltip.Trigger
          asChild={asChild}
          className={classNames(
            "focus-visible:outline-0",
            !asChild && "cursor-default",
            className,
          )}
          type="button" // don’t make the tooltip trigger submit forms
          aria-label={ariaLabel}
        >
          {children}
        </RadixTooltip.Trigger>
        <RadixTooltip.Portal container={portalContainer ?? undefined}>
          <RadixTooltip.Content
            side={side}
            align={align}
            className={classNames(
              "z-50 break-words rounded-sm border bg-background-secondary shadow-xs p-1 text-center text-xs",
              maxWidthClassName,
              contentClassName,
            )}
            role="tooltip"
            sideOffset={5}
          >
            {tip}
          </RadixTooltip.Content>
        </RadixTooltip.Portal>
      </RadixTooltip.Root>
    </RadixTooltip.Provider>
  );
}
