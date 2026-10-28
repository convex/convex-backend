import { ExternalLinkIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { useIsSafari } from "@common/lib/useIsSafari";
import { DisconnectedOverlay } from "./DisconnectOverlay";
import { useEffect, useState } from "react";

export function LocalDeploymentDisconnectOverlay() {
  const isSafari = useIsSafari();
  const isBrave = useIsBrave();

  return (
    <DisconnectedOverlay>
      {isSafari ? (
        <>
          <p className="mb-1">Safari blocks connections to localhost.</p>
          <p className="mb-4">
            We recommend using another browser when using local deployments.
          </p>
          <Button
            href="https://docs.convex.dev/cli/local-deployments#safari"
            variant="neutral"
            icon={<ExternalLinkIcon />}
            target="_blank"
          >
            Learn more
          </Button>
        </>
      ) : isBrave ? (
        <>
          <p className="mb-2">
            Brave blocks connections to localhost by default. We recommend using
            another browser or{" "}
            <Link
              href="https://docs.convex.dev/cli/local-deployments#brave"
              target="_blank"
              rel="noreferrer"
            >
              setting up Brave to allow localhost connections
            </Link>
            .
          </p>
          <Button
            href="https://docs.convex.dev/cli/local-deployments#brave"
            variant="neutral"
            icon={<ExternalLinkIcon />}
            target="_blank"
          >
            Learn more
          </Button>
        </>
      ) : (
        <>
          <p className="mb-2">
            Check that <code className="text-sm">npx convex dev</code> is
            running successfully.
          </p>
          <p>
            If you have multiple devices you use with this Convex project, the
            local deployment may be running on a different device, and can only
            be accessed on that machine.
          </p>
        </>
      )}
    </DisconnectedOverlay>
  );
}

function useIsBrave(): boolean {
  const [isBrave, setIsBrave] = useState(false);
  useEffect(() => {
    setIsBrave("brave" in navigator);
  }, []);
  return isBrave;
}
