import { Button } from "@ui/Button";
import { formatDate } from "@common/lib/format";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Checkbox } from "@ui/Checkbox";
import { useState } from "react";
import { Link } from "@ui/Link";
import { useCancelSubscription } from "api/billing";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { HelpTooltip } from "@ui/HelpTooltip";
import { OrbSubscriptionResponse, TeamResponse } from "generatedApi";
import startCase from "lodash/startCase";
import { BILLING_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { PlanCard } from "./PlanCard";

export function FreePlan({
  subscription,
  team,
  isLoading = false,
}: {
  subscription?: OrbSubscriptionResponse;
  team: TeamResponse;
  isLoading?: boolean;
}) {
  const [isSelfServeDowngradeModalOpen, setIsSelfServeDowngradeModalOpen] =
    useState(false);
  const [acceptedConsequences, setAcceptedConsequences] = useState(false);
  const cancelSubscription = useCancelSubscription(team.id);
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canCancelCustom = useHasCustomRolePermission(
    team.id,
    "billing:subscription:changePlan",
    BILLING_RESOURCE,
    false,
  );
  const canCancel = isTeamAdmin || canCancelCustom;
  return (
    <>
      <PlanCard
        selected={!subscription}
        plan={{
          id: "CONVEX_BASE",
          planType: "CONVEX_BASE",
          name: "Free",
          description: "For hobbyists and prototypes.",
          status: "active",
          seatPrice: 0,
        }}
        saleHeader="No credit card required"
        action={
          !subscription ? (
            <p className="flex h-8.5 items-center px-0.5 font-semibold">
              Current Plan
            </p>
          ) : typeof subscription.endDate === "number" ? (
            <p className="flex items-center gap-1 px-0.5 py-2 font-semibold">
              Next Billing Cycle{" "}
              <HelpTooltip>
                {`Your subscription has been canceled and will end on ${formatDate(new Date(subscription.endDate))}. You may resume the subscription before then to avoid losing access to features.`}
              </HelpTooltip>
            </p>
          ) : (
            <Button
              disabled={
                isLoading ||
                canCancel !== true ||
                team.managedBy === "vercel" ||
                subscription.plan.planType === "CONVEX_BUSINESS"
              }
              tip={
                canCancel === false
                  ? permissionDeniedTip(
                      "You do not have permission to modify the team subscription.",
                      "billing:subscription:changePlan",
                    )
                  : team.managedBy === "vercel"
                    ? `You can manage your subscription in ${startCase(team.managedBy)}.`
                    : subscription.plan.planType === "CONVEX_BUSINESS"
                      ? "Please contact support to change your plan."
                      : typeof subscription.endDate === "number"
                        ? `Your subscription has already been canceled and will end on ${formatDate(new Date(subscription.endDate))}. You may resume the subscription before then to avoid losing access to features.`
                        : undefined
              }
              variant="neutral"
              onClick={() => {
                setIsSelfServeDowngradeModalOpen(true);
              }}
            >
              Downgrade to Free
            </Button>
          )
        }
      />
      {isSelfServeDowngradeModalOpen && (
        <ConfirmationDialog
          onClose={() => setIsSelfServeDowngradeModalOpen(false)}
          dialogTitle="Downgrade to Free"
          dialogBody={
            <div className="flex flex-col gap-4">
              <p>Are you sure you want to downgrade to the Free plan?</p>
              <p>
                Once you downgrade, your team will lose access to all features
                that are not included in the Free plan at the end of the current
                billing period.
              </p>
              <p>
                If this team's{" "}
                <Link href={`/t/${team?.slug}/settings/usage`}>usage</Link>{" "}
                exceeds the Free plan limits, your projects may be automatically
                disabled.
              </p>
              <label className="mx-1 flex gap-2 text-sm">
                <Checkbox
                  className="mt-0.5"
                  checked={acceptedConsequences}
                  onChange={(e) =>
                    setAcceptedConsequences(e.currentTarget.checked)
                  }
                />{" "}
                By checking this box, I acknowledge my team may lose access to
                features or projects exceeding Free plan usage limits.
              </label>
            </div>
          }
          variant="danger"
          confirmText="Downgrade"
          disableConfirm={!acceptedConsequences}
          onConfirm={async () => {
            await cancelSubscription();
          }}
        />
      )}
    </>
  );
}
