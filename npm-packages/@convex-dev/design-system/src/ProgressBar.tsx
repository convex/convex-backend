import { cn } from "@ui/cn";

export function ProgressBar({
  fraction,
  ariaLabel,
  variant = "stripes",
  className,
}: {
  fraction: undefined | number;
  ariaLabel: string;
  variant?: "stripes" | "solid";
  className?: string;
}) {
  return (
    <div
      className={cn(
        "h-4 overflow-hidden rounded-full bg-background-tertiary",
        className,
      )}
      role="progressbar"
      aria-valuenow={fraction !== undefined ? fraction * 100 : undefined}
      aria-label={ariaLabel}
    >
      <div
        className="size-full overflow-hidden motion-safe:transition-[clip-path]"
        style={{
          clipPath:
            fraction !== undefined
              ? `polygon(0 0, ${fraction * 100}% 0, ${fraction * 100}% 100%, 0 100%)`
              : undefined,
        }}
      >
        <div
          className={cn("box-content size-full bg-util-accent pl-8", {
            "motion-safe:animate-[.5s_indeterminateProgressBar_infinite_linear]":
              variant === "stripes",
          })}
          style={{
            backgroundImage:
              variant === "stripes"
                ? "linear-gradient(45deg, rgba(255, 255, 255, 0.15) 25%, rgba(0, 0, 0, 0) 25%, rgba(0, 0, 0, 0) 50%, rgba(255, 255, 255, 0.15) 50%, rgba(255, 255, 255, 0.15) 75%, rgba(0, 0, 0, 0) 75%, rgba(0, 0, 0, 0))"
                : undefined,
            backgroundSize: "1rem",
          }}
        />
      </div>
    </div>
  );
}

export function ProgressBarWithPercent({
  fraction,
  variant,
  ariaLabel,
}: {
  fraction: number;
  variant: "stripes" | "solid";
  ariaLabel: string;
}) {
  const percent = Math.round(fraction * 100);
  return (
    <div className="flex items-center gap-3">
      <ProgressBar
        fraction={fraction}
        ariaLabel={ariaLabel}
        variant={variant}
        className="grow"
      />
      <span className="min-w-[4ch] text-right text-xs text-content-tertiary tabular-nums">
        {percent}%
      </span>
    </div>
  );
}
