import { ExclamationTriangleIcon } from "@radix-ui/react-icons";
import { Link } from "@ui/Link";

export function UsageLimitDisabledBanner({
  usageLimitsUri,
}: {
  usageLimitsUri: string;
}) {
  return (
    <div className="flex items-center justify-center gap-2 border-y bg-background-error px-4 py-2 text-center text-content-error">
      <ExclamationTriangleIcon className="shrink-0" />
      <span>
        This deployment is disabled because it exceeded a usage limit, and all
        function calls will fail. Review it on the{" "}
        <Link href={usageLimitsUri} className="font-medium">
          usage limits
        </Link>{" "}
        page.
      </span>
    </div>
  );
}
