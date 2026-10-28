import { useCallback, useContext } from "react";
import useSWR, { useSWRConfig } from "swr";
import { Id } from "system-udfs/convex/_generated/dataModel";
import { createDeploymentClient } from "@convex-dev/platform";
import type {
  ActiveDataSync,
  UsageLimitConfigResponse,
} from "@convex-dev/platform/deploymentApi";
import { useAdminKey, useDeploymentUrl } from "@common/lib/deploymentApi";
import { toast } from "@common/lib/utils";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import type {
  CurrentUsage,
  UsageLimit,
  UsageLimitConfig,
  UsageMetric,
  UsageSeedStatus,
} from "@common/features/settings/components/UsageLimits";

export function useUpdateEnvVars(): (
  changes: {
    name: string;
    value: string | null;
  }[],
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  return async (changes) => {
    const body = JSON.stringify({ changes });
    const res = await fetch(
      `${deploymentUrl}/api/update_environment_variables`,
      {
        method: "POST",
        headers: {
          Authorization: `Convex ${adminKey}`,
          "Content-Type": "application/json",
        },
        body,
      },
    );
    if (res.status !== 200) {
      const err = await res.json();
      reportHttpError("POST", res.url, err);
      toast("error", err.message);
    }
  };
}

export function useDeleteComponent() {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  return useCallback(
    async (id: Id<"_components">) => {
      const res = await fetch(`${deploymentUrl}/api/delete_component`, {
        method: "POST",
        headers: {
          Authorization: `Convex ${adminKey}`,
          "Content-Type": "application/json",
        },
        body: JSON.stringify({ componentId: id }),
      });
      if (res.status !== 200) {
        const err = await res.json();
        reportHttpError("DELETE", res.url, err);
        toast("error", err.message);
      }
    },
    [deploymentUrl, adminKey, reportHttpError],
  );
}

export function usePauseDeployment(): () => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  return async () => {
    const client = createDeploymentClient(deploymentUrl, adminKey);
    const { response } = await client.POST("/pause_deployment");
    if (response.status !== 200) {
      const err = await response.json();
      reportHttpError("POST", response.url, err);
      toast("error", err.message);
    } else {
      toast("success", "Deployment is now paused");
    }
  };
}

export function useUnpauseDeployment(): () => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  return async () => {
    const client = createDeploymentClient(deploymentUrl, adminKey);
    const { response } = await client.POST("/unpause_deployment");
    if (response.status !== 200) {
      const err = await response.json();
      reportHttpError("POST", response.url, err);
      toast("error", err.message);
    } else {
      toast("success", "Deployment is now running");
    }
  };
}

function toUsageLimit(response: UsageLimitConfigResponse): UsageLimit {
  return {
    id: response.id,
    metric: response.metric as UsageLimit["metric"],
    window: response.window as UsageLimit["window"],
    limitType: response.limitType as UsageLimit["limitType"],
    limit: response.limit,
    enabled: response.enabled,
  };
}

// Normalize an openapi-fetch error body into the { code, message } shape that
// reportHttpError / toast expect.
function normalizeUsageLimitError(error: unknown): {
  code: string;
  message: string;
} {
  if (error && typeof error === "object") {
    const { code, message } = error as { code?: unknown; message?: unknown };
    return {
      code: typeof code === "string" ? code : "UnknownError",
      message: typeof message === "string" ? message : "Something went wrong.",
    };
  }
  return { code: "UnknownError", message: "Something went wrong." };
}

function usageLimitsKey(deploymentUrl: string) {
  return ["usageLimits", deploymentUrl] as const;
}

export function useUsageLimits(): {
  usageLimits: UsageLimit[] | undefined;
  isLoading: boolean;
} {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  const { data, isLoading } = useSWR(
    usageLimitsKey(deploymentUrl),
    async () => {
      const client = createDeploymentClient(deploymentUrl, adminKey);
      const {
        data: body,
        error,
        response,
      } = await client.GET("/list_usage_limits");
      if (error || !response.ok || !body) {
        const normalized = normalizeUsageLimitError(error);
        reportHttpError("GET", response.url, normalized);
        toast("error", normalized.message);
        throw new Error(normalized.message);
      }
      return body.usageLimits.map(toUsageLimit);
    },
  );
  return { usageLimits: data, isLoading };
}

const SEED_STATUSES: UsageSeedStatus[] = [
  "pending",
  "partial",
  "complete",
  "failed",
];
function normalizeSeedStatus(value: string): UsageSeedStatus {
  return (SEED_STATUSES as string[]).includes(value)
    ? (value as UsageSeedStatus)
    : "complete";
}

function currentUsageKey(deploymentUrl: string) {
  return ["currentUsage", deploymentUrl] as const;
}

// `enabled` gates the fetch on the caller's `deployment:usage:view`
// permission — passing `false` (or a still-loading `undefined`) skips the
// request so we never hit `/get_current_usage` without the permission.
export function useCurrentUsage(enabled: boolean = true): {
  currentUsage: CurrentUsage | undefined;
  seedStatus: UsageSeedStatus | undefined;
  isLoading: boolean;
} {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  const { data, isLoading } = useSWR(
    enabled ? currentUsageKey(deploymentUrl) : null,
    async () => {
      const client = createDeploymentClient(deploymentUrl, adminKey);
      const {
        data: body,
        error,
        response,
      } = await client.GET("/get_current_usage");
      if (error || !response.ok || !body) {
        const normalized = normalizeUsageLimitError(error);
        reportHttpError("GET", response.url, normalized);
        toast("error", normalized.message);
        throw new Error(normalized.message);
      }
      const currentUsage: CurrentUsage = {};
      for (const [metric, { usage }] of Object.entries(body.metrics)) {
        currentUsage[metric as UsageMetric] = {
          day: usage.current_day,
          month: usage.current_month,
        };
      }
      return {
        currentUsage,
        seedStatus: normalizeSeedStatus(body.seedStatus),
      };
    },
    {
      refreshInterval: 5000,
    },
  );
  return {
    currentUsage: data?.currentUsage,
    seedStatus: data?.seedStatus,
    isLoading,
  };
}

export function useCreateUsageLimit(): (
  config: UsageLimitConfig,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  const { mutate } = useSWRConfig();
  return useCallback(
    async (config) => {
      const client = createDeploymentClient(deploymentUrl, adminKey);
      const { error, response } = await client.POST("/create_usage_limit", {
        body: config,
      });
      if (error || !response.ok) {
        const normalized = normalizeUsageLimitError(error);
        reportHttpError("POST", response.url, normalized);
        toast("error", normalized.message);
        // Throw so the caller can tell the save failed and keep its editor
        // open; the toast above has already surfaced the error to the user.
        throw new Error(normalized.message);
      }
      await mutate(usageLimitsKey(deploymentUrl));
    },
    [deploymentUrl, adminKey, reportHttpError, mutate],
  );
}

export function useUpdateUsageLimit(): (
  id: string,
  config: UsageLimitConfig,
) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  const { mutate } = useSWRConfig();
  return useCallback(
    async (id, config) => {
      const client = createDeploymentClient(deploymentUrl, adminKey);
      const { error, response } = await client.POST(
        "/update_usage_limit/{id}",
        {
          params: { path: { id } },
          body: config,
        },
      );
      if (error || !response.ok) {
        const normalized = normalizeUsageLimitError(error);
        reportHttpError("POST", response.url, normalized);
        toast("error", normalized.message);
        // Throw so the caller can tell the save failed and keep its editor
        // open; the toast above has already surfaced the error to the user.
        throw new Error(normalized.message);
      }
      await mutate(usageLimitsKey(deploymentUrl));
    },
    [deploymentUrl, adminKey, reportHttpError, mutate],
  );
}

export function useDeleteUsageLimit(): (id: string) => Promise<void> {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { reportHttpError } = useContext(DeploymentInfoContext);
  const { mutate } = useSWRConfig();
  return useCallback(
    async (id) => {
      const client = createDeploymentClient(deploymentUrl, adminKey);
      const { error, response } = await client.POST(
        "/delete_usage_limit/{id}",
        {
          params: { path: { id } },
        },
      );
      if (error || !response.ok) {
        const normalized = normalizeUsageLimitError(error);
        reportHttpError("POST", response.url, normalized);
        toast("error", normalized.message);
        return;
      }
      await mutate(usageLimitsKey(deploymentUrl));
    },
    [deploymentUrl, adminKey, reportHttpError, mutate],
  );
}

function activeDataSyncsKey(deploymentUrl: string) {
  return ["activeDataSyncs", deploymentUrl] as const;
}

// Fivetran drives the initial snapshot in a tight loop, so poll often enough
// that its progress visibly advances while the page is open.
const ACTIVE_DATA_SYNCS_REFRESH_MS = 15 * 1000;

// Syncs that called `/data/sync` within the past 3 days, across every
// integration. `enabled` gates the fetch on the two things the endpoint
// requires: the caller's `deployment:data:view` permission and the team's
// streaming export entitlement. Returns `undefined` until the first response
// lands, and on failure — this backs a passive status indicator, so a broken
// poll shows nothing rather than toasting every 15 seconds.
export function useActiveDataSyncs(
  enabled: boolean,
): ActiveDataSync[] | undefined {
  const deploymentUrl = useDeploymentUrl();
  const adminKey = useAdminKey();
  const { data } = useSWR(
    enabled ? activeDataSyncsKey(deploymentUrl) : null,
    async () => {
      const client = createDeploymentClient(deploymentUrl, adminKey);
      const { data: body, response } = await client.GET(
        "/data/list_active_syncs",
      );
      return response.ok && body ? body.syncs : undefined;
    },
    { refreshInterval: ACTIVE_DATA_SYNCS_REFRESH_MS },
  );
  return data;
}
