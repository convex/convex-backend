import { Button } from "@ui/Button";
import { Combobox } from "@ui/Combobox";
import { useState } from "react";
import { Sheet } from "@ui/Sheet";
import { Callout } from "@ui/Callout";
import { TeamResponse } from "generatedApi";
import { Loading } from "@ui/Loading";
import { ReferralsBenefits } from "components/referral/ReferralsBenefits";
import { Link } from "@ui/Link";
import { MAX_REFERRALS } from "./Referrals";

type TeamEligibilityError =
  | "paid_subscription"
  | "already_redeemed"
  | "not_admin";

export function RedeemReferralForm({
  referralCode,
  teams,
  selectedTeam,
  onTeamSelect,
  onSubmit,
  isTeamSelectorShown,
  onShowTeamSelector,
  teamEligibility,
  isChef,
}: {
  referralCode:
    | {
        valid: false;
      }
    | {
        valid: true;
        teamName: string;
        exhausted: boolean;
      }
    | undefined;
  teams: TeamResponse[] | undefined;
  selectedTeam: TeamResponse | null;
  onTeamSelect: (team: TeamResponse) => void;
  onSubmit: () => Promise<void>;
  isTeamSelectorShown: boolean;
  onShowTeamSelector: () => void;
  teamEligibility:
    | undefined
    | { eligible: true }
    | { eligible: false; reason: TeamEligibilityError };
  isChef: boolean;
}) {
  const [isSubmitting, setIsSubmitting] = useState(false);

  return (
    <div className="flex w-screen items-center justify-center">
      <Sheet className="flex w-full max-w-prose flex-col gap-2">
        {referralCode === undefined ? (
          <>
            <h3>Redeem your referral code</h3>
            <Loading fullHeight={false} className="h-80" />
          </>
        ) : !referralCode.valid ? (
          <CodeError
            title="Invalid referral code"
            description="Oh no, the code you used is invalid."
            isChef={isChef}
          />
        ) : referralCode.exhausted ? (
          <CodeError
            title="Referral code exhausted"
            description={
              <>
                Oh no, the code from <strong>{referralCode.teamName}</strong>{" "}
                has been redeemed more than {MAX_REFERRALS} times and is no
                longer valid.
              </>
            }
            isChef={isChef}
          />
        ) : (
          <>
            <h3>Redeem your referral code</h3>

            <p className="text-content-primary">
              Thanks to the code from <strong>{referralCode.teamName}</strong>,
              you will get the following resources for free on top of your free
              plan limits:
            </p>

            <ul className="mt-4 mb-3 grid gap-x-2 gap-y-4 sm:grid-cols-2">
              <ReferralsBenefits />
            </ul>

            {teams === undefined ? (
              <Loading fullHeight={false} className="h-28" />
            ) : (
              <form
                onSubmit={async (e) => {
                  e.preventDefault();

                  setIsSubmitting(true);
                  try {
                    await onSubmit();
                  } finally {
                    setIsSubmitting(false);
                  }
                }}
                className="flex flex-col gap-4"
              >
                {isTeamSelectorShown ||
                selectedTeam === null ||
                teamEligibility?.eligible === false ? (
                  <div className="flex flex-col gap-1">
                    <Combobox
                      labelHidden={false}
                      options={teams.map((t) => ({
                        label: t.name,
                        value: t.slug,
                      }))}
                      label="Apply the code to team"
                      selectedOption={selectedTeam?.slug ?? null}
                      setSelectedOption={(slug) => {
                        if (slug !== null) {
                          const team = teams?.find((t) => t.slug === slug);
                          if (team) {
                            onTeamSelect(team);
                          }
                        }
                      }}
                      disableSearch
                    />
                  </div>
                ) : (
                  <div className="flex flex-wrap items-center gap-2">
                    <p className="grow text-content-secondary">
                      These free resources will be added to the team{" "}
                      <strong className="font-semibold">
                        {selectedTeam.name}
                      </strong>
                      .{" "}
                    </p>
                    <Button
                      variant="neutral"
                      onClick={onShowTeamSelector}
                      size="xs"
                    >
                      Change team
                    </Button>
                  </div>
                )}

                {teamEligibility?.eligible === false && (
                  <Callout variant="error" className="my-0">
                    {teamEligibilityErrorMessage(teamEligibility.reason)}
                  </Callout>
                )}

                <div>
                  <Button
                    type="submit"
                    disabled={!selectedTeam || !teamEligibility?.eligible}
                    loading={isSubmitting || teamEligibility === undefined}
                  >
                    {isChef
                      ? "Get my free Chef tokens and Convex resources"
                      : "Get my free resources"}
                  </Button>
                </div>
              </form>
            )}
          </>
        )}
      </Sheet>
    </div>
  );
}

function teamEligibilityErrorMessage(error: TeamEligibilityError) {
  switch (error) {
    case "paid_subscription":
      return "You cannot redeem a referral code on a team that has a paid subscription.";
    case "already_redeemed":
      return "This team has already redeemed a referral code.";
    case "not_admin":
      return "You must be an admin of the team to redeem a referral code.";
    default: {
      error satisfies never;
      throw new Error(`Unknown team eligibility error: ${error}`);
    }
  }
}

function CodeError({
  title,
  description,
  isChef,
}: {
  title: string;
  description: React.ReactNode;
  isChef: boolean;
}) {
  return (
    <>
      <h3>{title}</h3>
      <p>{description}</p>
      <p>
        No worries, you can still use another referral link later. In the
        meantime, you can get started using Convex with the{" "}
        <Link href="https://www.convex.dev/pricing" target="_blank">
          default limits
        </Link>{" "}
        and also refer others to increase your quota.
      </p>

      <div>
        {isChef ? (
          <Button href="https://chef.convex.dev">Start Building</Button>
        ) : (
          <Button href="/">Start Building</Button>
        )}
      </div>
    </>
  );
}
