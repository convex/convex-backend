import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { Button } from "@ui/Button";
import {
  OrbSubscriptionResponse,
  PlanResponse,
  TeamResponse,
} from "generatedApi";
import { useRouter } from "next/router";
import { useChangeSubscription } from "api/billing";
import { useState } from "react";
import {
  DowngradePlanDialog,
  UpgradePlanDialog,
} from "components/billing/planCards/ChangePlanDialogs";
import { planNameMap } from "components/billing/planCards/PlanCard";
import startCase from "lodash/startCase";
import { BILLING_RESOURCE } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { SelfServePlan } from "./SelfServePlan";

export function OrbSelfServePlan({
  orbSub,
  plan,
  team,
  isLoading = false,
}: {
  orbSub?: OrbSubscriptionResponse;
  plan: PlanResponse;
  team: TeamResponse;
  isLoading?: boolean;
}) {
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canChangePlanCustom = useHasCustomRolePermission(
    team.id,
    "billing:subscription:changePlan",
    BILLING_RESOURCE,
    false,
  );
  const canChangePlan = isTeamAdmin || canChangePlanCustom;

  const { push } = useRouter();

  const upgrade = () => {
    void push(
      {
        pathname: "/t/[team]/settings/billing",
        query: {
          team: team.slug,
          upgradePlan: plan.id,
        },
      },
      undefined,
      { shallow: true },
    );
  };

  const isDowngrade =
    (orbSub?.plan.planType === "CONVEX_PROFESSIONAL" &&
      plan.planType === "CONVEX_STARTER_PLUS") ||
    (orbSub?.plan.planType === "CONVEX_BUSINESS" &&
      (plan.planType === "CONVEX_STARTER_PLUS" ||
        plan.planType === "CONVEX_PROFESSIONAL"));

  const changePlan = useChangeSubscription(team.id);

  const [isChangingPlan, setIsChangingPlan] = useState(false);

  const newPlanName = plan.planType
    ? planNameMap[plan.planType] || plan.name
    : plan.name;
  const missingRequiredPaymentMethod =
    orbSub &&
    orbSub?.paymentMethod === null &&
    orbSub?.plan.planType === "CONVEX_PROFESSIONAL" &&
    plan.planType === "CONVEX_STARTER_PLUS";

  const isOnBusinessPlan = orbSub?.plan.planType === "CONVEX_BUSINESS";

  return (
    <>
      <SelfServePlan
        currentPlan={orbSub?.plan.id}
        plan={plan}
        percentOff={0}
        isLoading={isLoading}
        action={
          orbSub?.plan.planType === plan.planType ||
          orbSub?.plan.id === plan.id ? (
            <p className="flex h-8.5 items-center px-2 font-semibold">
              Current Plan
            </p>
          ) : orbSub ? (
            <Button
              loading={isLoading}
              tip={
                canChangePlan === false
                  ? permissionDeniedTip(
                      "You do not have permission to modify the team subscription.",
                      "billing:subscription:changePlan",
                    )
                  : team.managedBy === "vercel"
                    ? `You can manage your subscription in ${startCase(team.managedBy)}.`
                    : isOnBusinessPlan
                      ? "Please contact support to change your plan."
                      : missingRequiredPaymentMethod
                        ? "Add a payment method in the settings below to switch to this plan."
                        : undefined
              }
              onClick={() => {
                setIsChangingPlan(true);
              }}
              disabled={
                canChangePlan !== true ||
                missingRequiredPaymentMethod ||
                team.managedBy === "vercel" ||
                isOnBusinessPlan
              }
              variant="neutral"
            >
              {isDowngrade
                ? `Downgrade to ${newPlanName}`
                : `Upgrade to ${newPlanName}`}
            </Button>
          ) : (
            <Button
              onClick={() => upgrade()}
              loading={isLoading}
              disabled={canChangePlan !== true || team.managedBy === "vercel"}
              variant="neutral"
              tip={
                canChangePlan === false
                  ? permissionDeniedTip(
                      "You do not have permission to modify the team subscription.",
                      "billing:subscription:changePlan",
                    )
                  : team.managedBy === "vercel"
                    ? `You can manage your subscription in ${startCase(team.managedBy)}.`
                    : undefined
              }
            >
              Upgrade to {newPlanName}
            </Button>
          )
        }
      />
      {isChangingPlan &&
        (isDowngrade ? (
          <DowngradePlanDialog
            onClose={() => setIsChangingPlan(false)}
            onConfirm={() => changePlan({ newPlanId: plan.id })}
            newPlan={plan}
            team={team}
          />
        ) : (
          <UpgradePlanDialog
            onClose={() => setIsChangingPlan(false)}
            onConfirm={(newPlanId, promoCode) =>
              changePlan({ newPlanId, promoCode })
            }
            newPlan={plan}
            team={team}
          />
        ))}
    </>
  );
}
