import React from "react";
import { ReferralState, TeamResponse } from "generatedApi";
import { useTeamOrbSubscription } from "api/billing";
import { Sheet } from "@ui/Sheet";
import { Callout } from "@ui/Callout";
import { useReferralState } from "api/referrals";
import { Loading } from "@ui/Loading";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { Link } from "@ui/Link";
import { ReferralsBenefits } from "./ReferralsBenefits";
import { ReferralProgress } from "./ReferralProgress";

// Keep in sync with MAX_REFERRALS_BONUS in big_brain_lib/src/model/referrals.rs
export const MAX_REFERRALS = 5;

export function Referrals({ team }: { team: TeamResponse }) {
  const { subscription } = useTeamOrbSubscription(team.id);
  const isPaidPlan =
    subscription === undefined ? undefined : subscription !== null;

  const referralState = useReferralState(team.id);

  return (
    <ReferralsInner
      isPaidPlan={isPaidPlan}
      referralCode={team.referralCode}
      referralState={referralState}
    />
  );
}

export function ReferralsInner({
  isPaidPlan,
  referralCode,
  referralState,
}: {
  isPaidPlan: boolean | undefined;
  referralCode: string;
  referralState: ReferralState | undefined;
}) {
  const sourceReferralTeamName = referralState?.referredBy;
  const isFreePlan = isPaidPlan === false;

  return (
    <>
      <h2>Referrals</h2>

      {isPaidPlan && (
        <Callout variant="upsell">
          <div className="flex flex-col gap-1">
            <p className="font-semibold">
              Thank you for subscribing to Convex!
            </p>
            <p className="max-w-prose">
              Referral benefits only apply on the free plan, so your team no
              longer receives additional resources from referring friends or
              being referred. Your friends can still get free Convex resources
              by using your referral link below.
            </p>
          </div>
        </Callout>
      )}

      <Sheet>
        <h3>Refer friends and earn free Convex resources</h3>
        <p className="mt-1 max-w-prose">
          Each time you refer someone, both of your teams get the following
          benefits on top of your{" "}
          <Link href="https://www.convex.dev/pricing" target="_blank">
            free plan limits
          </Link>
          . Referral benefits only apply while your team is on the free plan —
          they no longer apply if you upgrade to a paid plan.
        </p>

        <div className="my-4 flex items-center gap-4">
          <hr className="grow" />
          <p className="text-content-secondary">
            For each team that you refer (up to {MAX_REFERRALS} teams):
          </p>
          <hr className="grow" />
        </div>

        <ul className="mt-4 mb-3 grid gap-x-2 gap-y-4 sm:grid-cols-2 lg:grid-cols-3">
          <ReferralsBenefits />
        </ul>

        <hr className="my-4" />

        <ReferralLink referralCode={referralCode} />

        {sourceReferralTeamName ===
        undefined ? null : sourceReferralTeamName === null ? (
          <p className="text-content-secondary">
            You have not been referred.{" "}
            {isPaidPlan === false &&
              "Use a referral link to get free Convex resources!"}
          </p>
        ) : (
          <p className="text-content-secondary">
            You have been referred by{" "}
            <strong className="font-semibold">{sourceReferralTeamName}</strong>.
          </p>
        )}
      </Sheet>

      <Sheet>
        <div className="flex flex-col gap-4 text-sm">
          <div className="flex flex-col gap-4 py-2 md:grow md:flex-row md:items-center md:justify-between md:gap-2">
            <h3>Your referrals</h3>
            <div className="flex flex-col gap-1 xl:max-w-prose xl:grow xl:flex-row-reverse xl:items-center xl:gap-2">
              {isFreePlan && referralState && (
                <ReferralProgress referralState={referralState} />
              )}
            </div>
          </div>
          {referralState === undefined ? (
            <Loading fullHeight={false} className="h-48" />
          ) : (
            <div className="flex flex-col">
              {referralState.referrals.length === 0 ? (
                <p className="text-content-secondary">
                  No referrals yet. Share your referral link to get started!
                </p>
              ) : (
                referralState.referrals.map((teamName, index) => (
                  <div
                    key={index}
                    className="flex items-center justify-between border-b py-3 last:border-b-0"
                  >
                    <span className="text-sm text-content-primary">
                      {teamName}
                    </span>
                  </div>
                ))
              )}
            </div>
          )}
        </div>
      </Sheet>
    </>
  );
}

function ReferralLink({ referralCode }: { referralCode: string }) {
  return (
    <div className="my-6 flex max-w-72 flex-col gap-1">
      <span className="text-sm">Your referral link:</span>
      <CopyTextButton text={`https://convex.dev/referral/${referralCode}`} />
    </div>
  );
}
