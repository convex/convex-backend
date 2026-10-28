import { cn } from "@ui/cn";
import type { RegionName } from "generatedApi";
import { PRIMARY_REGION } from "lib/regions";

/**
 * Warns about the surcharge outside the primary region. Always
 * rendered so that picking a region doesn't shift the surrounding layout.
 */
export function RegionPricingWarning({
  region,
}: {
  region: RegionName | null;
}) {
  const show = region !== null && region !== PRIMARY_REGION;

  return (
    <p
      className={cn(
        "mt-2 text-xs text-content-warning transition-opacity",
        show ? "opacity-100" : "opacity-0 select-none",
        // `relative` fixes a weird browser bug where Safari would sometimes ignore opacity-0
        "relative",
      )}
      inert={!show}
      aria-hidden={!show}
    >
      Usage in this region has a 30% regional surcharge.
    </p>
  );
}
