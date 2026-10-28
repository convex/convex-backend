import { useContext, useState } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  Cross2Icon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { Link } from "@ui/Link";

// Little toast to prompt users who are trying out Convex before creating
// an account about the Convex cloud product.
export function ConvexCloudReminderToast() {
  const { useCurrentDeployment } = useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();
  const isAnonymousDevelopment =
    deployment?.name?.startsWith("anonymous-") ||
    deployment?.name?.startsWith("tryitout-");
  const [isExpanded, setIsExpanded] = useState(false);
  const [isDismissed, setIsDismissed] = useState(false);

  if (!isAnonymousDevelopment || isDismissed) {
    return null;
  }

  return (
    // Positioned in the bottom left corner, high enough to not block the
    // sidebar collapse button.
    <div className="absolute bottom-12 left-4 z-50">
      <div
        className="w-96 rounded-lg border border-purple-500 bg-background-secondary shadow-lg dark:border-purple-200"
        role="region"
        aria-label="Convex cloud notice"
      >
        <div className="flex items-center gap-1 p-1">
          <Button
            variant="unstyled"
            className="flex flex-1 cursor-pointer items-center gap-2 rounded-md p-1 text-left text-sm font-medium text-purple-700 hover:bg-background-tertiary focus-visible:ring-2 focus-visible:ring-purple-500 focus-visible:outline-hidden dark:text-purple-200 dark:focus-visible:ring-purple-200"
            onClick={() => setIsExpanded(!isExpanded)}
            aria-expanded={isExpanded}
            aria-controls="anonymous-development-details"
          >
            {isExpanded ? (
              <ChevronDownIcon className="size-4 shrink-0" />
            ) : (
              <ChevronRightIcon className="size-4 shrink-0" />
            )}
            <span>Enjoying Convex? Ready to deploy your app?</span>
          </Button>
          <Button
            variant="unstyled"
            className="shrink-0 cursor-pointer rounded-full p-1 text-purple-700 hover:bg-purple-100 focus-visible:ring-2 focus-visible:ring-purple-500 focus-visible:outline-hidden dark:text-purple-200 dark:hover:bg-purple-900 dark:focus-visible:ring-purple-200"
            onClick={() => setIsDismissed(true)}
            aria-label="Dismiss"
          >
            <Cross2Icon className="size-4" />
          </Button>
        </div>
        {isExpanded && (
          <div
            id="anonymous-development-details"
            className="flex flex-col gap-2 border-t border-purple-500/30 px-4 py-3 text-sm text-content-primary dark:border-purple-200/30"
          >
            <p>You are currently trying out Convex by running it locally.</p>
            <p>
              If you're ready to deploy your app and share it with the world or
              want to access more features with the cloud product, create a
              Convex account and automatically link this deployment:
            </p>
            <p className="inline-flex items-center gap-2">
              Run this in your terminal:
              <CopyTextButton text="npx convex login" />
            </p>
            <Link href="https://docs.convex.dev" target="_blank" externalIcon>
              Learn more about Convex
            </Link>
          </div>
        )}
      </div>
    </div>
  );
}
