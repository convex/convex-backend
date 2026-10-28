import { ExternalLinkIcon } from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import { Spinner } from "@ui/Spinner";

export type ConvexStatusIndicator = "none" | "minor" | "major" | "critical";

export interface ConvexStatus {
  indicator: ConvexStatusIndicator;
  description: string;
}

const STATUS_PAGE_URL = "https://status.convex.dev";

export function ConvexStatusWidget({ status }: { status?: ConvexStatus }) {
  return (
    <a
      href={STATUS_PAGE_URL}
      target="_blank"
      rel="noreferrer"
      className={cn(
        "flex items-center gap-2 text-sm hover:underline",
        !status && "animate-fadeInFromLoading",
      )}
    >
      {!status ? (
        <>
          <div>
            <Spinner className="animate-fadeInFromLoading" />
          </div>
          <span className="animate-fadeInFromLoading text-content-secondary">
            Loading system status...
          </span>
          <ExternalLinkIcon className="animate-fadeInFromLoading" />
        </>
      ) : (
        <>
          <span className="relative flex size-3 shrink-0">
            {status.indicator === "none" && (
              <span className="absolute inline-flex size-full animate-ping rounded-full bg-content-success opacity-75" />
            )}
            <span
              className={cn(
                "relative inline-flex size-3 rounded-full",
                // eslint-disable-next-line no-restricted-syntax
                status.indicator === "none" && "bg-content-success",
                status.indicator === "minor" && "bg-yellow-500",
                // eslint-disable-next-line no-restricted-syntax
                status.indicator === "major" && "bg-content-errorSecondary",
                // eslint-disable-next-line no-restricted-syntax
                status.indicator === "critical" && "bg-content-errorSecondary",
              )}
            />
          </span>
          <span className="flex items-center gap-1">
            {status.description}
            <ExternalLinkIcon />
          </span>
        </>
      )}
    </a>
  );
}
