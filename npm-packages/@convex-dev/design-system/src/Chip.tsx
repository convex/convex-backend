import { Cross2Icon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";
import { ReactNode, useState } from "react";

const CHIP_CLASSES =
  "flex max-w-full items-center gap-1 rounded-sm border px-1.5 py-1 text-xs text-content-primary shadow-xs";

type ChipProps = {
  children: ReactNode;
  className?: string;
} & (
  | { onRemove?: never; removeLabel?: never; removeTip?: never }
  | {
      onRemove: () => void;
      removeLabel: string;
      removeTip?: ReactNode;
    }
);

export function Chip(props: ChipProps) {
  if (props.onRemove === undefined) {
    return (
      <span className={cn(CHIP_CLASSES, props.className)}>
        <span className="truncate">{props.children}</span>
      </span>
    );
  }
  return <RemovableChip {...props} />;
}

function RemovableChip({
  children,
  className,
  onRemove,
  removeLabel,
  removeTip,
}: {
  children: ReactNode;
  className?: string;
  onRemove: () => void;
  removeLabel: string;
  removeTip?: ReactNode;
}) {
  // Only a real pointer move arms the red remove state. A chip added from the
  // keyboard can render under a pointer resting where the user last clicked,
  // and `:hover` alone would mark it for removal.
  const [isArmed, setIsArmed] = useState(false);
  return (
    <span
      className="flex max-w-full"
      onPointerMove={() => setIsArmed(true)}
      onPointerLeave={() => setIsArmed(false)}
    >
      <Button
        variant="unstyled"
        aria-label={removeLabel}
        tip={removeTip}
        onClick={onRemove}
        className={cn(
          CHIP_CLASSES,
          "cursor-pointer transition-colors",
          isArmed &&
            "border-content-error bg-background-error text-content-error shadow-none",
          className,
        )}
      >
        <span className="truncate">{children}</span>
        <Cross2Icon className="size-3 shrink-0" />
      </Button>
    </span>
  );
}
