import { joinUrlPath } from "@common/lib/helpers/joinUrlPath";

async function sleep(ms: number) {
  return new Promise((resolve) => {
    setTimeout(resolve, ms);
  });
}

const MAX_RETRIES = 3;
const MAX_RETRIES_DELAY_MS = 500;

export type CheckDeploymentResult = {
  allowedOps: string[];
  isReadOnly: boolean;
} | null;

export async function checkDeploymentInfo(
  adminKey: string,
  deploymentUrl: string,
): Promise<CheckDeploymentResult> {
  let retries = 0;
  while (retries < MAX_RETRIES) {
    try {
      const resp = await fetch(
        joinUrlPath(deploymentUrl, "/api/check_admin_key"),
        {
          method: "GET",
          headers: {
            "Content-Type": "application/json",
            Authorization: `Convex ${adminKey}`,
            "Convex-Client": "dashboard-0.0.0",
          },
        },
      );
      if (resp.ok) {
        try {
          const body = await resp.json();
          return {
            allowedOps: body.allowedOps ?? [],
            isReadOnly: body.isReadOnly ?? false,
          };
        } catch {
          // Old backend that doesn't return JSON with allowedOps
          return { allowedOps: [], isReadOnly: false };
        }
      }
      if (resp.status === 404) {
        // Endpoint doesn't exist on this backend — allow all operations
        return { allowedOps: [], isReadOnly: false };
      }
    } catch {
      // Do nothing
    }
    await sleep(MAX_RETRIES_DELAY_MS);
    retries++;
  }
  return null;
}
