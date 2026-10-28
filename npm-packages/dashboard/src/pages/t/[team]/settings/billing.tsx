import { Sheet } from "@ui/Sheet";
import { LocalDevCallout } from "@common/elements/LocalDevCallout";
import { Callout } from "@ui/Callout";
import { Button } from "@ui/Button";
import { useTeamMembers } from "api/teams";
import { useListPlans, useTeamOrbSubscription } from "api/billing";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { BILLING_RESOURCE } from "lib/permissions";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { Link } from "@ui/Link";
import { useRouter } from "next/router";
import { TeamResponse } from "generatedApi";
import { Plans } from "components/billing/Plans";
import { SubscriptionOverview } from "components/billing/SubscriptionOverview";
import { ErrorBoundary, captureMessage } from "@sentry/nextjs";
import { cn } from "@ui/cn";
import { UpgradePlanContentContainer } from "components/billing/UpgradePlanContent";
import { useProfile } from "api/profile";
import { ChevronLeftIcon } from "@radix-ui/react-icons";
import { Loading } from "@ui/Loading";
import { planNameMap } from "components/billing/planCards/PlanCard";
import { OpenInVercel } from "components/OpenInVercel";
import startCase from "lodash/startCase";
import { PromoCodeModalContainer } from "components/billing/PromoCodeModal";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";

export { getServerSideProps } from "lib/ssr";

function Billing({ team }: { team: TeamResponse }) {
  const { promos } = useLaunchDarkly();
  const { subscription: orbSub, isLoading: isOrbSubLoading } =
    useTeamOrbSubscription(team.id);

  const router = useRouter();

  const members = useTeamMembers(team.id);
  const hasAdminPermissions = useIsCurrentMemberTeamAdmin();
  // Mirror the gate `OrbSelfServePlan` uses on the Upgrade button — otherwise
  // a custom-role member with `billing:subscription:changePlan` can click
  // Upgrade and watch the page do nothing because `showUpgrade` stays false.
  const canChangePlanCustom = useHasCustomRolePermission(
    team.id,
    "billing:subscription:changePlan",
    BILLING_RESOURCE,
    false,
  );
  const canChangePlan = hasAdminPermissions || canChangePlanCustom === true;
  const myProfile = useProfile();
  const orbPlans = useListPlans(team.id);
  const selectedPlan = orbPlans.plans?.find(
    (p) => p.id === router.query.upgradePlan,
  );

  const newPlanName = selectedPlan?.planType
    ? planNameMap[selectedPlan.planType] || selectedPlan.name
    : selectedPlan?.name;

  const showUpgrade =
    selectedPlan && orbSub?.plan.id !== selectedPlan.id && canChangePlan;
  const promoCode =
    typeof router.query.promoCode === "string"
      ? router.query.promoCode
      : undefined;

  return (
    <div className="-mx-6 flex grow flex-col">
      {promos && promoCode !== undefined && (
        <PromoCodeModalContainer code={promoCode} initialTeam={team} />
      )}
      <div className="sticky top-0 z-10 -mt-6 flex items-center gap-2 bg-background-primary p-6">
        {showUpgrade && (
          <Button
            icon={<ChevronLeftIcon className="size-5" />}
            tip="Back to Plans"
            onClick={() =>
              void router.push(
                {
                  pathname: "/t/[team]/settings/billing",
                  query: {
                    team: team.slug,
                  },
                },
                undefined,
                { shallow: true },
              )
            }
            size="xs"
            variant="neutral"
            className="text-content-secondary"
            inline
          />
        )}
        <h2>Billing</h2>
      </div>
      {team.managedBy === "vercel" && (
        <Callout className="mx-6 mb-4" variant="upsell">
          <div className="flex w-full items-center justify-between gap-4">
            <div>
              This team is managed by {startCase(team.managedBy)}. You must
              manage billing through the {startCase(team.managedBy)} dashboard.
            </div>
            <OpenInVercel team={team} />
          </div>
        </Callout>
      )}
      <ErrorBoundary fallback={BillingErrorFallback}>
        <div className="relative min-h-0 flex-1 overflow-x-hidden">
          {!isOrbSubLoading && orbSub !== undefined ? (
            <div
              className={cn(
                "flex size-full min-h-0 gap-6 transition-transform duration-500 motion-reduce:transition-none",
                showUpgrade
                  ? "-translate-x-[calc(100%+1.5rem)]"
                  : "translate-x-0",
              )}
            >
              <div
                className={cn(
                  "scrollbar flex w-full shrink-0 grow flex-col gap-4 overflow-y-auto px-6 pr-2 pb-6",
                  showUpgrade ? "pointer-events-none select-none" : "",
                )}
                // @ts-expect-error https://github.com/facebook/react/issues/17157
                inert={showUpgrade ? "inert" : undefined}
              >
                <div className="flex w-full min-w-[20rem] flex-col gap-4">
                  <Sheet className="flex flex-col gap-4 text-sm">
                    <h3>Plans</h3>
                    <Plans team={team} subscription={orbSub || undefined} />
                    <div className="text-center text-content-secondary">
                      Compare all plan features on the{" "}
                      <Link
                        href="https://convex.dev/plans"
                        passHref
                        target="_blank"
                      >
                        pricing page
                      </Link>
                    </div>
                  </Sheet>
                  <SubscriptionOverview
                    team={team}
                    hasAdminPermissions={hasAdminPermissions}
                    subscription={isOrbSubLoading ? undefined : orbSub}
                  />
                  <LocalDevCallout
                    tipText="Tip: Run this to enable audit logs locally:"
                    command={`just big-brain-tool-dev entitlement grant add --team-entitlement audit_log_retention_days --team-id ${team.id} --reason "local" 90 --for-real`}
                  />
                </div>
              </div>
              <div
                className={cn(
                  "scrollbar flex w-full shrink-0 grow flex-col gap-4 overflow-auto px-6",
                  !showUpgrade ? "pointer-events-none select-none" : "",
                )}
                // @ts-expect-error https://github.com/facebook/react/issues/17157
                inert={!showUpgrade ? "inert" : undefined}
              >
                {showUpgrade && selectedPlan && (
                  <Sheet className="scrollbar max-h-full w-2xl overflow-y-auto">
                    <h3 className="mb-4">Upgrade to {newPlanName}</h3>
                    <UpgradePlanContentContainer
                      name={myProfile?.name}
                      email={myProfile?.email}
                      team={team}
                      numMembers={members?.length || 1}
                      plan={selectedPlan}
                      onUpgradeComplete={() => {
                        void router.push(
                          {
                            pathname: "/t/[team]/settings/billing",
                            query: {
                              team: team.slug,
                            },
                          },
                          undefined,
                          { shallow: true },
                        );
                      }}
                    />
                  </Sheet>
                )}
              </div>
            </div>
          ) : (
            <Loading className="mx-6 size-full" />
          )}
        </div>
      </ErrorBoundary>
    </div>
  );
}

export function BillingPage() {
  return (
    <TeamSettingsLayout page="billing" Component={Billing} title="Billing" />
  );
}

export default withAuthenticatedPage(BillingPage);

function BillingErrorFallback({ eventId }: { eventId: string | null }) {
  captureMessage("BillingErrorFallback triggered", "info");
  return (
    <Callout variant="error" className="w-fit">
      <div className="flex flex-col gap-2">
        <p>We encountered an error loading your billing information.</p>
        <p>
          {" "}
          Please try again or contact us at{" "}
          <Link
            href="mailto:support@convex.dev"
            passHref
            className="items-center"
          >
            support@convex.dev
          </Link>{" "}
          for support with this issue.
        </p>
        {eventId !== null && <div>Event ID: {eventId}</div>}{" "}
      </div>
    </Callout>
  );
}
