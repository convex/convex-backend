import { useState } from "react";
import {
  CheckCircledIcon,
  CrossCircledIcon,
  ExternalLinkIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Modal } from "@ui/Modal";
import { Spinner } from "@ui/Spinner";
import type { DirectoryResponse } from "generatedApi";
import {
  useGenerateDirectorySyncConfigurationLink,
  useGetDirectorySync,
} from "api/directorySync";

const POLL_INTERVAL_MS = 3000;

type ConnectionStatus = {
  tone: "waiting" | "error" | "linked";
  label: string;
  description: string;
};

function connectionStatus(
  directory: DirectoryResponse | undefined,
): ConnectionStatus {
  if (directory?.linked) {
    return {
      tone: "linked",
      label: "Directory connected",
      description:
        "Convex is syncing your directory. Groups and members appear as they arrive, which can take up to an hour.",
    };
  }
  switch (directory?.state) {
    case "invalid_credentials":
      return {
        tone: "error",
        label: "Invalid credentials",
        description:
          "Your identity provider rejected the directory credentials. Correct them there to finish connecting the directory.",
      };
    case "deleting":
      return {
        tone: "error",
        label: "Directory is being deleted",
        description:
          "This directory is being removed from your identity provider. Configure a new one to sync your user directory.",
      };
    default:
      // A directory that exists but has not linked is the same wait as one the
      // identity provider has not created yet: either way the work left is in
      // the portal tab, and nothing here distinguishes how far along it is.
      return {
        tone: "waiting",
        label: "Waiting for connection",
        description:
          "Finish setting up the directory with your identity provider in the tab that just opened.",
      };
  }
}

function StatusRow({ status }: { status: ConnectionStatus }) {
  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center gap-2 font-medium">
        {status.tone === "waiting" && <Spinner className="ml-0 shrink-0" />}
        {status.tone === "linked" && (
          <CheckCircledIcon className="size-4 shrink-0 text-content-success" />
        )}
        {status.tone === "error" && (
          <CrossCircledIcon className="size-4 shrink-0 text-content-error" />
        )}
        {status.label}
      </div>
      <p className="text-sm text-content-secondary">{status.description}</p>
    </div>
  );
}

export function ConnectDirectoryDialog({
  teamId,
  onClose,
}: {
  teamId: number;
  onClose: () => void;
}) {
  const [hasOpenedPortal, setHasOpenedPortal] = useState(false);
  const [isGeneratingLink, setIsGeneratingLink] = useState(false);
  const generateLink = useGenerateDirectorySyncConfigurationLink(teamId);

  const { data } = useGetDirectorySync(teamId, {
    // Reading the latest response is what stops the poll: a linked directory
    // is the end of this flow, and the dialog can sit open on it.
    refreshInterval: (latest) =>
      hasOpenedPortal && !latest?.directory?.linked ? POLL_INTERVAL_MS : 0,
  });
  const directory = data?.directory ?? undefined;
  const status = connectionStatus(directory);
  const isLinked = hasOpenedPortal && status.tone === "linked";

  const openPortal = async () => {
    // The tab has to be opened by the click itself. `generateLink` awaits a
    // round trip, and by the time it resolves the browser no longer counts
    // this as user-initiated and blocks the popup.
    const portalTab = window.open("", "_blank");
    setIsGeneratingLink(true);
    try {
      const result = await generateLink();
      if (!result?.link) {
        portalTab?.close();
        return;
      }
      if (portalTab) {
        portalTab.location.href = result.link;
      } else if (!window.open(result.link, "_blank")) {
        // Nothing opened, so leave the member on the intro and its button
        // rather than sending them to wait on a tab that is not there.
        return;
      }
      setHasOpenedPortal(true);
    } catch {
      // `useBBMutation` has already reported the failure; the blank tab is
      // the only thing left to clean up.
      portalTab?.close();
    } finally {
      setIsGeneratingLink(false);
    }
  };

  const action = isLinked ? (
    <Button onClick={onClose}>Continue</Button>
  ) : hasOpenedPortal ? null : (
    <Button
      icon={<ExternalLinkIcon />}
      loading={isGeneratingLink}
      onClick={openPortal}
    >
      Configure Identity Provider
    </Button>
  );

  return (
    <Modal title="Configure Directory Sync" onClose={onClose} size="sm">
      {hasOpenedPortal ? (
        <div className="flex flex-col gap-2 pb-3" aria-live="polite">
          <StatusRow status={status} />
          {status.tone === "waiting" && (
            <p className="text-sm text-content-secondary">
              If you've completed the configuration steps, you may close this
              dialog and come back later once the directory has been synced.
            </p>
          )}
        </div>
      ) : (
        <div className="flex flex-col gap-4 pb-3 text-content-primary">
          <p>
            To enable Directory Sync, you'll first need to connect and configure
            an external identity provider so it can sync your user directory
            into Convex.
          </p>
          <p>
            Once a directory is synced, you'll be able to map its groups to
            Convex roles and review the role changes Directory Sync would make
            before turning it on.
          </p>
        </div>
      )}
      {action && <div className="flex w-full justify-end">{action}</div>}
    </Modal>
  );
}
