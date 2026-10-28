import { useContext } from "react";
import { DeploymentSettingsLayout } from "@common/layouts/DeploymentSettingsLayout";
import {
  DeploymentInfoContext,
  PermissionsContext,
} from "@common/lib/deploymentContext";
import {
  NoPermissionMessage,
  PermissionDeniedTip,
} from "@common/elements/NoPermissionMessage";
import { Sheet } from "@ui/Sheet";
import {
  UsageLimits,
  computeUnbilledMetrics,
} from "@common/features/settings/components/UsageLimits";
import {
  useUsageLimits,
  useCurrentUsage,
  useCreateUsageLimit,
  useUpdateUsageLimit,
  useDeleteUsageLimit,
} from "@common/features/settings/lib/api";

export function UsageLimitsView() {
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canView = useIsOperationAllowed("ViewUsageLimits");
  const canViewUsage = useIsOperationAllowed("ViewUsage");
  const canWrite = useIsOperationAllowed("WriteUsageLimits");

  return (
    <DeploymentSettingsLayout page="usage-limits">
      {canView ? (
        <UsageLimitsContent canWrite={canWrite} canViewUsage={canViewUsage} />
      ) : (
        <Sheet className="max-w-3xl py-12">
          <NoPermissionMessage
            message="You do not have permission to view usage limits."
            missingPermission="deployment:usageLimits:view"
          />
        </Sheet>
      )}
    </DeploymentSettingsLayout>
  );
}

function UsageLimitsContent({
  canWrite,
  canViewUsage,
}: {
  canWrite: boolean;
  canViewUsage: boolean;
}) {
  const { useCurrentDeployment, useCurrentTeam, useTeamPlanType, teamsURI } =
    useContext(DeploymentInfoContext);
  const deployment = useCurrentDeployment();
  const team = useCurrentTeam();
  const planType = useTeamPlanType(team?.id ?? null);
  // Business and Enterprise share the "CONVEX_BUSINESS" plan type.
  const isBusinessPlan = planType === "CONVEX_BUSINESS";
  // Dedicated (DXXXX) deployment classes start with "d" (e.g. "d1024").
  const isDedicated =
    deployment?.kind === "cloud" && deployment.class.startsWith("d");
  // Billing tiers are a Convex Cloud concept; self-hosted deployments have no
  // plan or billing, so the "unbilled metric" callouts don't apply there.
  const unbilledMetrics =
    deployment?.kind === "cloud"
      ? computeUnbilledMetrics({ isBusinessPlan, isDedicated })
      : {};
  // Billing lives on the team settings page, which only exists for cloud
  // deployments; self-hosted deployments have no billing, so no billing notes.
  const billingUri =
    deployment?.kind === "cloud" ? `${teamsURI}/settings/billing` : undefined;

  const { usageLimits, isLoading } = useUsageLimits();
  const { currentUsage, seedStatus } = useCurrentUsage(canViewUsage);
  const createUsageLimit = useCreateUsageLimit();
  const updateUsageLimit = useUpdateUsageLimit();
  const deleteUsageLimit = useDeleteUsageLimit();

  return (
    <UsageLimits
      usageLimits={usageLimits ?? []}
      isLoading={isLoading}
      canWrite={canWrite}
      writePermissionTip={
        <PermissionDeniedTip
          message="You do not have permission to modify usage limits."
          action="deployment:usageLimits:write"
        />
      }
      unbilledMetrics={unbilledMetrics}
      currentUsage={currentUsage}
      seedStatus={seedStatus}
      deploymentCreateTime={deployment?.createTime}
      deploymentType={deployment?.deploymentType}
      billingUri={billingUri}
      onCreate={createUsageLimit}
      onUpdate={updateUsageLimit}
      onDelete={deleteUsageLimit}
    />
  );
}
