import { useRef } from "react";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import type { CreditResponse, InvoiceResponse } from "generatedApi";
import { BILLING_RESOURCE, Permissioned } from "lib/permissions";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "./roles";
import { useBBMutation, useBBQuery } from "./api";

// Subscription info is readable by all team members; but some details may not be available depending on whether the member has `billing:view`.
export function useTeamOrbSubscription(teamId?: number) {
  const {
    data: subscription,
    error,
    isLoading,
  } = useBBQuery({
    path: "/teams/{team_id}/get_orb_subscription",
    pathParams: { team_id: teamId?.toString() ?? "" },
    swrOptions: { refreshInterval: 0, keepPreviousData: false },
  });
  if (error) {
    return { isLoading, subscription: null, hasError: true };
  }
  return { isLoading, subscription, hasError: false };
}

export function useTeamPrices(teamId: number) {
  const { dashboardShowCashUsage } = useLaunchDarkly();
  const canView = useHasCustomRolePermission(
    teamId,
    "billing:view",
    BILLING_RESOURCE,
    true,
  );
  const result = useBBQuery({
    path: "/teams/{team_id}/get_prices",
    pathParams: {
      team_id:
        dashboardShowCashUsage && canView === true ? teamId.toString() : "",
    },
    swrOptions: { keepPreviousData: false },
  });
  return { ...result, canView };
}

export function useCreateSetupIntent(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/create_setup_intent",
    pathParams: { team_id: teamId.toString() },
  });
}

export function useCreateSubscription(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/create_subscription",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast:
      "Congratulations! Your Convex subscription has been activated.",
  });
}

export function useChangeSubscription(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/change_subscription_plan",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Subscription changed.",
  });
}

export function useCancelSubscription(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/cancel_orb_subscription",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Subscription canceled.",
  });
}

export function useResumeSubscription(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/unschedule_cancel_orb_subscription",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Subscription resumed.",
  });
}

export function useUpdatePaymentMethod(teamId: number) {
  return useBBMutation({
    method: "put",
    path: "/teams/{team_id}/update_payment_method",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Payment method updated.",
  });
}

export function useUpdateBillingContact(teamId: number) {
  return useBBMutation({
    method: "put",
    path: "/teams/{team_id}/update_billing_contact",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Billing contact updated.",
  });
}

export function useUpdateBillingAddress(teamId: number) {
  return useBBMutation({
    method: "put",
    path: "/teams/{team_id}/update_billing_address",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_orb_subscription",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Billing address updated.",
  });
}

export function useGetCoupon(
  teamId: number,
  planId: string,
  promoCode?: string,
) {
  const { data, error, isLoading } = useBBQuery({
    path: "/teams/{team_id}/get_discounted_plan/{plan_id}/{promo_code}",
    pathParams: {
      team_id: teamId.toString(),
      plan_id: planId,
      promo_code: promoCode || "",
    },
    swrOptions: {
      refreshInterval: 0,
      shouldRetryOnError: false,
    },
  });

  if (error) {
    return {
      isLoading: false,
      errorMessage: "Failed to load coupon. Please try again.",
    };
  }

  return { coupon: data, isLoading: !!promoCode && isLoading };
}

export function useHasFailedPayment(
  teamId?: number,
): Permissioned<{ hasFailedPayment: boolean }> {
  const canView = useHasCustomRolePermission(
    teamId,
    "billing:view",
    BILLING_RESOURCE,
    true,
  );
  const { data, isLoading } = useBBQuery({
    path: "/teams/{team_id}/has_failed_payment",
    pathParams: {
      team_id: canView === true ? (teamId?.toString() ?? "") : "",
    },
    swrOptions: { refreshInterval: 1000 * 60 },
  });
  if (canView === undefined) return { status: "loading" };
  if (canView === false) {
    return { status: "denied", deniedAction: "billing:view" };
  }
  if (isLoading) return { status: "loading" };
  return {
    status: "ok",
    data: { hasFailedPayment: data?.hasFailedPayment ?? false },
  };
}

export function useListInvoices(
  teamId?: number,
  limit?: number,
): Permissioned<{
  invoices: InvoiceResponse[];
  hasMore: boolean;
  isRefreshing: boolean;
}> {
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canViewCustom = useHasCustomRolePermission(
    teamId,
    "billing:invoices:view",
    BILLING_RESOURCE,
    false,
  );
  const canView =
    canViewCustom === undefined ? undefined : isTeamAdmin || canViewCustom;
  const { data, isLoading } = useBBQuery({
    path: "/teams/{team_id}/list_invoices",
    pathParams: {
      team_id: canView === true ? (teamId?.toString() ?? "") : "",
    },
    queryParams: { limit },
    swrOptions: { refreshInterval: 1000 * 60 },
  });
  // Hold onto the last successful response so callers can keep rendering
  // the existing list while a refetch (e.g. raising `limit`) is in flight.
  const lastDataRef = useRef<typeof data>(undefined);
  if (data !== undefined) lastDataRef.current = data;
  const effectiveData = data ?? lastDataRef.current;

  if (canView === undefined) return { status: "loading" };
  if (canView === false) {
    return { status: "denied", deniedAction: "billing:invoices:view" };
  }
  if (isLoading && effectiveData === undefined) return { status: "loading" };
  return {
    status: "ok",
    // Don't show test invoices from before we launched Orb billing.
    data: {
      invoices:
        effectiveData?.invoices.filter(
          (invoice) => new Date(invoice.invoiceDate) >= new Date("2024-04-29"),
        ) ?? [],
      hasMore:
        limit !== undefined &&
        effectiveData !== undefined &&
        effectiveData.invoices.length >= limit,
      // True when we're displaying previous data while a fetch for the
      // current `limit` is still in flight.
      isRefreshing: isLoading && effectiveData !== undefined,
    },
  };
}

export function useListPlans(teamId?: number) {
  const { data, error, isLoading } = useBBQuery({
    path: "/teams/{team_id}/list_active_plans",
    pathParams: { team_id: teamId?.toString() ?? "" },
    swrOptions: { refreshInterval: 0 },
  });
  if (error) {
    throw error;
  }
  return { plans: data?.plans, isLoading };
}

export function useGetCurrentSpend(
  teamId: number | null,
): Permissioned<{ totalCents: number | undefined }> {
  const canView = useHasCustomRolePermission(
    teamId ?? undefined,
    "billing:view",
    BILLING_RESOURCE,
    true,
  );
  const { data, isLoading } = useBBQuery({
    path: "/teams/{team_id}/get_current_spend",
    pathParams: {
      team_id: canView === true ? (teamId?.toString() ?? "") : "",
    },
    swrOptions: { refreshInterval: 1000 * 60 },
  });
  if (canView === undefined) return { status: "loading" };
  if (canView === false) {
    return { status: "denied", deniedAction: "billing:view" };
  }
  if (isLoading) return { status: "loading" };
  return { status: "ok", data: { totalCents: data?.totalCents } };
}

type CreditsResult = Permissioned<CreditResponse[]> & {
  refreshCredits: () => Promise<void>;
};

export function useListCredits(teamId: number | null): CreditsResult {
  const canView = useHasCustomRolePermission(
    teamId ?? undefined,
    "billing:view",
    BILLING_RESOURCE,
    true,
  );
  const { data, isLoading, mutate } = useBBQuery({
    path: "/teams/{team_id}/list_credits",
    pathParams: {
      team_id: canView === true ? (teamId?.toString() ?? "") : "",
    },
    // Credits are granted out of band and don't change over the life of a page
    // view, so don't poll Orb for them.
    swrOptions: { refreshInterval: 0 },
  });
  const refreshCredits = async () => {
    await mutate();
  };
  if (canView === undefined) {
    return { status: "loading", refreshCredits } as const;
  }
  if (canView === false) {
    return {
      status: "denied",
      deniedAction: "billing:view",
      refreshCredits,
    } as const;
  }
  if (isLoading) return { status: "loading", refreshCredits } as const;
  // The response is paged-shaped, but the server returns every credit today.
  return {
    status: "ok",
    data: data?.items ?? [],
    refreshCredits,
  } as const;
}

export type SpendingLimits = {
  disableThresholdCents: number | null;
  state: null | "Running" | "Disabled" | "Warning";
  warningThresholdCents: number | null;
};

export function useGetSpendingLimits(
  teamId: number | null,
): Permissioned<SpendingLimits | undefined> {
  const canView = useHasCustomRolePermission(
    teamId ?? undefined,
    "billing:view",
    BILLING_RESOURCE,
    true,
  );
  const { data, isLoading } = useBBQuery({
    path: "/teams/{team_id}/get_spending_limits",
    pathParams: {
      team_id: canView === true ? (teamId?.toString() ?? "") : "",
    },
    swrOptions: { refreshInterval: 1000 * 60 },
  });
  if (canView === undefined) return { status: "loading" };
  if (canView === false) {
    return { status: "denied", deniedAction: "billing:view" };
  }
  if (isLoading) return { status: "loading" };
  return {
    status: "ok",
    data:
      data === undefined
        ? undefined
        : {
            // The `?? null` checks are only necessary to fix an issue in the
            // OpenAPI codegen, the server always sends a value.
            state: data.state ?? null,
            disableThresholdCents: data.disableThresholdCents ?? null,
            warningThresholdCents: data.warningThresholdCents ?? null,
          },
  };
}

export function useSetSpendingLimit(teamId: number) {
  return useBBMutation({
    path: "/teams/{team_id}/set_spending_limit",
    pathParams: { team_id: teamId.toString() },
    mutateKey: "/teams/{team_id}/get_spending_limits",
    mutatePathParams: { team_id: teamId.toString() },
    successToast: "Spending limit updated.",
  });
}
