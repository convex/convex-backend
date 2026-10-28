import { useContext } from "react";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { toast } from "@common/lib/utils";
import { createDeploymentClient } from "@convex-dev/platform";
import {
  CreateLogStreamArgs,
  UpdateLogStreamArgs,
} from "@convex-dev/platform/deploymentApi";
import { useAdminKey, useDeploymentUrl } from "./deploymentApi";

// `openapi-fetch` consumes the response body when it produces the
// `{ data, error, response }` tuple, so the deserialized error lives
// on the `error` field — calling `response.json()` ourselves throws
// "body stream already read". Normalize whatever shape comes back
// into the `{ code, message }` form `reportHttpError` expects.
function normalizeError(
  error: unknown,
  fallback: string,
): { code: string; message: string } {
  if (typeof error === "string") {
    return { code: "Unknown", message: error };
  }
  if (typeof error === "object" && error !== null) {
    const e = error as { code?: unknown; message?: unknown };
    return {
      code: typeof e.code === "string" ? e.code : "Unknown",
      message: typeof e.message === "string" ? e.message : fallback,
    };
  }
  return { code: "Unknown", message: fallback };
}

export function useCreateLogStream(): (
  args: CreateLogStreamArgs,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);

  // Throws on non-200 so callers can render the error inline; we still
  // report it to Sentry but no longer toast (the form surfaces the
  // message next to the Save button).
  return async (args: CreateLogStreamArgs) => {
    const client = createDeploymentClient(deploymentUrl, adminKey);
    const { error, response } = await client.POST("/create_log_stream", {
      body: args,
    });
    if (response.status !== 200) {
      const normalized = normalizeError(error, "Failed to create log stream.");
      reportHttpError("POST", response.url, normalized);
      throw new Error(normalized.message);
    }
  };
}

export function useUpdateLogStream(): (
  id: string,
  update: UpdateLogStreamArgs,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);

  return async (id: string, update: UpdateLogStreamArgs) => {
    const client = createDeploymentClient(deploymentUrl, adminKey);
    const { error, response } = await client.POST("/update_log_stream/{id}", {
      params: {
        path: {
          id,
        },
      },
      body: update,
    });
    if (response.status !== 200) {
      const normalized = normalizeError(error, "Failed to update log stream.");
      reportHttpError("POST", response.url, normalized);
      throw new Error(normalized.message);
    }
  };
}

/** Resolves to whether the delete succeeded; failures are already toasted. */
export function useDeleteLogStream(): (id: string) => Promise<boolean> {
  const { reportHttpError } = useContext(DeploymentInfoContext);
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();

  return async (id: string) => {
    const client = createDeploymentClient(deploymentUrl, adminKey);
    const { error, response } = await client.POST("/delete_log_stream/{id}", {
      params: {
        path: {
          id,
        },
      },
    });
    if (response.status !== 200) {
      const normalized = normalizeError(error, "Failed to delete log stream.");
      reportHttpError("DELETE", response.url, normalized);
      toast("error", normalized.message);
      return false;
    }
    return true;
  };
}

export function useRotateWebhookSecret(): (id: string) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);

  return async (id: string) => {
    const client = createDeploymentClient(deploymentUrl, adminKey);
    const { error, response } = await client.POST(
      "/rotate_webhook_secret/{id}",
      {
        params: {
          path: {
            id,
          },
        },
      },
    );
    if (response.status !== 200) {
      const normalized = normalizeError(
        error,
        "Failed to rotate webhook secret.",
      );
      reportHttpError("POST", response.url, normalized);
      toast("error", normalized.message);
    }
  };
}
