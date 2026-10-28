import { z } from "zod";

const currentDeploymentSchema = z.object({
  name: z.string(),
  url: z.string().url(),
  adminKey: z.string(),
});

export type CurrentDeployment = z.infer<typeof currentDeploymentSchema>;

/**
 * When the dashboard is served by the CLI (anonymous mode), the same server
 * exposes the credentials of the current deployment at /api/current_deployment.
 * Returns null if the endpoint doesn't exist (e.g. in the self-hosted
 * dashboard, which returns a 404) so the caller can fall back to other
 * mechanisms.
 */
export async function fetchCurrentDeployment(): Promise<CurrentDeployment | null> {
  let resp: Response;
  try {
    resp = await fetch("/api/current_deployment");
  } catch {
    return null;
  }
  if (!resp.ok) {
    return null;
  }
  try {
    return currentDeploymentSchema.parse(await resp.json());
  } catch {
    return null;
  }
}
