import { PropsWithChildren } from "react";
import { Tooltip, TooltipSide } from "./Tooltip";
import { QuestionMarkCircledIcon } from "@radix-ui/react-icons";

export function HelpTooltip({
  children,
  tipSide,
  maxWidthClassName,
}: PropsWithChildren<{
  tipSide?: TooltipSide;
  maxWidthClassName?: string;
}>) {
  return (
    <Tooltip
      tip={children}
      side={tipSide}
      aria-label="Show help"
      maxWidthClassName={maxWidthClassName}
    >
      <QuestionMarkCircledIcon className="text-content-tertiary" />
    </Tooltip>
  );
}
