import { Checkbox } from "@ui/Checkbox";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Spinner } from "@ui/Spinner";
import { TextInput } from "@ui/TextInput";
import { useGetCoupon } from "api/billing";
import { useTeamMembers } from "api/teams";
import { PriceSummary } from "components/billing/PriceSummary";
import { debounceDurationMs } from "components/billing/UpgradePlanContent";
import { planNameMap } from "components/billing/planCards/PlanCard";
import { PlanResponse, TeamResponse } from "generatedApi";
import { Link } from "@ui/Link";
import { useState } from "react";
import { useDebounce } from "react-use";

export function DowngradePlanDialog({
  onClose,
  onConfirm,
  newPlan,
  team,
}: {
  onClose: () => void;
  onConfirm: () => Promise<void>;
  newPlan: PlanResponse;
  team: TeamResponse;
}) {
  const [acceptedConsequences, setAcceptedConsequences] = useState(false);
  const newPlanName = newPlan.planType
    ? planNameMap[newPlan.planType] || newPlan.name
    : newPlan.name;
  return (
    <ConfirmationDialog
      onClose={onClose}
      dialogTitle={`Downgrade to ${newPlanName}`}
      dialogBody={
        <div className="flex flex-col gap-4">
          <p>Are you sure you want to downgrade to the {newPlanName} plan?</p>
          <p>
            Once you downgrade, your team will lose access to all features that
            are not included in the {newPlanName} plan{" "}
            {newPlan.planType === "CONVEX_STARTER"
              ? "at the end of the current billing period."
              : "immediately."}
          </p>
          {newPlan.planType === "CONVEX_STARTER" && (
            <p>
              If this team's{" "}
              <Link href={`/t/${team?.slug}/settings/usage`}>usage</Link>{" "}
              exceeds the {newPlanName} plan limits, your projects may be
              automatically disabled.
            </p>
          )}
          <label className="mx-1 flex gap-2 text-sm">
            <Checkbox
              className="mt-0.5"
              checked={acceptedConsequences}
              onChange={(e) => setAcceptedConsequences(e.currentTarget.checked)}
            />{" "}
            By checking this box, I acknowledge my team may lose access to
            features
            {newPlan.planType === "CONVEX_STARTER" &&
              ` and projects exceeding ${newPlanName} plan usage limits`}
            .
          </label>
        </div>
      }
      variant="danger"
      confirmText="Downgrade"
      disableConfirm={!acceptedConsequences}
      onConfirm={onConfirm}
    />
  );
}

export function UpgradePlanDialog({
  onClose,
  onConfirm,
  newPlan,
  team,
}: {
  onClose: () => void;
  onConfirm: (planId: string, promoCode?: string) => Promise<void>;
  newPlan: PlanResponse;
  team: TeamResponse;
}) {
  const [promoCode, setPromoCode] = useState("");
  const [debouncedPromoCode, setDebouncedPromoCode] = useState(promoCode);
  useDebounce(
    () => {
      setDebouncedPromoCode(promoCode);
    },
    debounceDurationMs,
    [promoCode],
  );

  const couponData = useGetCoupon(
    team.id,
    newPlan.id,
    debouncedPromoCode || "",
  );

  const teamMembers = useTeamMembers(team.id);
  const newPlanName = newPlan.planType
    ? planNameMap[newPlan.planType] || newPlan.name
    : newPlan.name;
  return (
    <ConfirmationDialog
      onClose={onClose}
      dialogTitle={`Upgrade to ${newPlanName}`}
      dialogBody={
        <div className="flex flex-col gap-6">
          <PriceSummary
            plan={newPlan}
            teamMemberDiscountPct={couponData.coupon?.percentOff ?? 0}
            numMembers={teamMembers?.length || 1}
            // If we're here, we're upgrading from one paid plan to another paid plan,
            // so we always set requiresPaymentMethod to true so that
            // the information about how much the user will be charged is shown.
            requiresPaymentMethod
            couponDurationInMonths={undefined}
            isUpgrading
            teamManagedBy={
              team.managedBy === "vercel" ? team.managedBy : undefined
            }
          />
          {newPlan.planType === "CONVEX_PROFESSIONAL" && (
            <div className="flex max-w-64 items-center gap-2">
              <TextInput
                label="Promo code"
                placeholder="Enter a promo code"
                onChange={(e) => setPromoCode(e.target.value)}
                value={promoCode}
                id="promoCode"
                error={couponData.errorMessage}
              />
              {couponData.isLoading && (
                <span data-testid="loading-spinner" className="mt-4">
                  <Spinner />
                </span>
              )}
            </div>
          )}
        </div>
      }
      disableConfirm={
        team.managedBy === "vercel" ||
        couponData.isLoading ||
        promoCode !== debouncedPromoCode
      }
      variant="primary"
      confirmText="Upgrade"
      onConfirm={() =>
        onConfirm(
          couponData.coupon?.planId ?? newPlan.id,
          debouncedPromoCode || undefined,
        )
      }
    />
  );
}
