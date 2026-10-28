import { useCallback, useContext } from "react";
import {
  useAdminKey,
  useDeploymentAuthHeader,
  useDeploymentUrl,
} from "@common/lib/deploymentApi";
import { toast } from "@common/lib/utils";
import { CompletedExport } from "system-udfs/convex/_system/frontend/common";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { logDeploymentEvent } from "convex-analytics";
import { reportHttpError } from "hooks/fetching";
import { ConnectedDeploymentContext } from "@common/lib/deploymentContext";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";

// The response is a `Content-Disposition: attachment`, so clicking a link to it
// starts a download instead of navigating away from the dashboard.
function startDownload(url: string) {
  const link = document.createElement("a");
  link.href = url;
  document.body.append(link);
  link.click();
  link.remove();
}

// A download is a plain navigation, which can't carry an `Authorization`
// header, so the credential has to travel in the URL. Under the
// `ephemeralZipExportToken` flag we mint a short-lived token scoped to this one
// snapshot instead of putting the long-lived admin key there.
export function useDownloadZipExport(
  format: CompletedExport["format"],
): (snapshotId: Id<"_exports">) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const authHeader = useDeploymentAuthHeader();
  const { ephemeralZipExportToken } = useLaunchDarkly();
  return useCallback(
    async (snapshotId: Id<"_exports">) => {
      if (format?.format !== "zip") {
        throw new Error("expected zip");
      }
      const exportUrl = `${deploymentUrl}/api/export/zip/${snapshotId}`;
      if (!ephemeralZipExportToken) {
        startDownload(`${exportUrl}?${new URLSearchParams({ adminKey })}`);
        return;
      }
      const res = await fetch(`${exportUrl}/token`, {
        method: "POST",
        headers: { Authorization: authHeader },
      });
      if (!res.ok) {
        const err = await res.json();
        reportHttpError("POST", res.url, err);
        toast("error", err.message);
        return;
      }
      const { token } = await res.json();
      startDownload(`${exportUrl}?${new URLSearchParams({ token })}`);
    },
    [deploymentUrl, adminKey, authHeader, format, ephemeralZipExportToken],
  );
}

export function useCancelImport(): (
  id: Id<"_snapshot_imports">,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  return useCallback(
    async (id: Id<"_snapshot_imports">) => {
      const res = await fetch(`${deploymentUrl}/api/cancel_import`, {
        method: "POST",
        headers: {
          Authorization: `Convex ${adminKey}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ importId: id }),
      });
      if (res.status !== 200) {
        const err = await res.json();
        reportHttpError("DELETE", res.url, err);
        toast("error", err.message);
      }
    },
    [deploymentUrl, adminKey],
  );
}

export function useCancelExport(): (id: Id<"_exports">) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  return useCallback(
    async (id: Id<"_exports">) => {
      const res = await fetch(`${deploymentUrl}/api/export/cancel/${id}`, {
        method: "POST",
        headers: { Authorization: authHeader },
      });
      if (!res.ok) {
        const err = await res.json();
        reportHttpError("POST", res.url, err);
        toast("error", err.message);
      }
    },
    [deploymentUrl, authHeader],
  );
}

export function useConfirmImport(): (
  id: Id<"_snapshot_imports">,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  return useCallback(
    async (importId: Id<"_snapshot_imports">) => {
      const url = `${deploymentUrl}/api/perform_import`;
      const res = await fetch(url, {
        method: "POST",
        headers: {
          Authorization: `Convex ${adminKey}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ importId }),
      });
      if (res.status !== 200) {
        const err = await res.json();
        reportHttpError("DELETE", res.url, err);
        toast("error", err.message);
      }
    },
    [deploymentUrl, adminKey],
  );
}

export function useLogDeploymentEvent() {
  const deployment = useContext(ConnectedDeploymentContext);
  if (!deployment) {
    throw Error("Must be used inside a loaded connected deployment!");
  }
  const deploymentUrl = useDeploymentUrl();
  const authHeader = useDeploymentAuthHeader();
  return useCallback(
    (msg: string, props: object | null = null) => {
      logDeploymentEvent(msg, deploymentUrl, authHeader, props);
    },
    [deploymentUrl, authHeader],
  );
}

export const useUpdateCanonicalUrl = (
  requestDestination: "convexCloud" | "convexSite",
) => {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  return useCallback(
    async (url: string | null) => {
      const res = await fetch(`${deploymentUrl}/api/update_canonical_url`, {
        method: "POST",
        headers: {
          Authorization: `Convex ${adminKey}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({
          requestDestination,
          url,
        }),
      });
      if (!res.ok) {
        const err = await res.json();
        toast("error", err.message);
      }
    },
    [adminKey, deploymentUrl, requestDestination],
  );
};
