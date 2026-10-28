import {
  useListCredits,
  useListPlans,
  useTeamOrbSubscription,
} from "api/billing";
import { useTeams } from "api/teams";
import { formatUsd, toast } from "@common/lib/utils";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { Combobox } from "@ui/Combobox";
import { Loading } from "@ui/Loading";
import { Link } from "@ui/Link";
import { Modal } from "@ui/Modal";
import { TeamResponse } from "generatedApi";
import { useRouter } from "next/router";
import { useEffect, useMemo, useState } from "react";

export type PromoDetails = {
  code: string;
  description: string;
  creditAmount: number;
  expirationTime: number;
  creditValidityDays: number;
};

export type PromoLookupState =
  | { status: "loading" }
  | { status: "error"; error: string }
  | { status: "success"; promo: PromoDetails };

type TeamPlan = "loading" | "error" | "free" | "paid";

export function PromoCodeModal({
  promoState,
  teams,
  selectedTeam,
  teamPlan,
  starterUpgradeUrl,
  isRedeeming,
  redemptionError,
  onSelectTeam,
  onRedeem,
  onClose,
}: {
  promoState: PromoLookupState;
  teams: TeamResponse[] | undefined;
  selectedTeam: TeamResponse | null;
  teamPlan: TeamPlan;
  starterUpgradeUrl?: string;
  isRedeeming: boolean;
  redemptionError?: string;
  onSelectTeam: (team: TeamResponse) => void;
  onRedeem: () => Promise<void>;
  onClose: () => void;
}) {
  return (
    <Modal title="Redeem account credit" onClose={onClose}>
      <div className="flex flex-col gap-4 pt-2">
        {promoState.status === "loading" ? (
          <Loading className="h-32" fullHeight={false} />
        ) : promoState.status === "error" ? (
          <Callout variant="error" className="m-0">
            {promoState.error}
          </Callout>
        ) : (
          <PromoRedemptionDetails
            promo={promoState.promo}
            teams={teams}
            selectedTeam={selectedTeam}
            teamPlan={teamPlan}
            starterUpgradeUrl={starterUpgradeUrl}
            isRedeeming={isRedeeming}
            redemptionError={redemptionError}
            onSelectTeam={onSelectTeam}
            onRedeem={onRedeem}
            onClose={onClose}
          />
        )}
      </div>
    </Modal>
  );
}

function PromoRedemptionDetails({
  promo,
  teams,
  selectedTeam,
  teamPlan,
  starterUpgradeUrl,
  isRedeeming,
  redemptionError,
  onSelectTeam,
  onRedeem,
  onClose,
}: {
  promo: PromoDetails;
  teams: TeamResponse[] | undefined;
  selectedTeam: TeamResponse | null;
  teamPlan: TeamPlan;
  starterUpgradeUrl?: string;
  isRedeeming: boolean;
  redemptionError?: string;
  onSelectTeam: (team: TeamResponse) => void;
  onRedeem: () => Promise<void>;
  onClose: () => void;
}) {
  return (
    <>
      <div className="flex flex-col gap-1">
        <p className="font-semibold">{promo.description}</p>
        <p className="text-sm text-content-secondary">
          {formatUsd(promo.creditAmount)} in account credit. The credit expires{" "}
          {promo.creditValidityDays} day
          {promo.creditValidityDays === 1 ? "" : "s"} after redemption.
        </p>
      </div>

      {teams === undefined ? (
        <Loading className="h-20" fullHeight={false} />
      ) : (
        <div className="flex flex-col gap-1">
          <Combobox
            label="Apply credit to team"
            labelHidden={false}
            options={teams.map((team) => ({
              label: team.name,
              value: team.slug,
            }))}
            selectedOption={selectedTeam?.slug ?? null}
            setSelectedOption={(slug) => {
              const team = teams.find((candidate) => candidate.slug === slug);
              if (team) {
                onSelectTeam(team);
              }
            }}
            disableSearch
          />
        </div>
      )}

      {redemptionError && (
        <Callout variant="error" className="m-0">
          {redemptionError}
        </Callout>
      )}

      {selectedTeam && teamPlan === "error" && (
        <Callout variant="error" className="m-0">
          Unable to load this team&apos;s billing status. Please try again.
        </Callout>
      )}

      {selectedTeam && teamPlan === "free" && (
        <p className="text-sm text-content-secondary">
          Promo codes for account credit can only be applied on Starter or
          higher.
          {starterUpgradeUrl && (
            <>
              {" "}
              <Link
                href={starterUpgradeUrl}
                target="_blank"
                rel="noopener noreferrer"
                externalIcon
              >
                Upgrade to Starter
              </Link>{" "}
              to redeem this credit.
            </>
          )}
        </p>
      )}

      <div className="flex justify-end gap-2">
        <Button variant="neutral" onClick={onClose}>
          Cancel
        </Button>
        {selectedTeam && (
          <Button
            onClick={() => void onRedeem()}
            disabled={teamPlan !== "paid"}
            loading={teamPlan === "loading" || isRedeeming}
            tip={
              teamPlan === "free"
                ? "Upgrade this team to Starter or higher to redeem this credit."
                : teamPlan === "error"
                  ? "Billing status must load successfully before this credit can be redeemed."
                  : undefined
            }
          >
            Redeem {formatUsd(promo.creditAmount)} credit
          </Button>
        )}
      </div>
    </>
  );
}

export function PromoCodeModalContainer({
  code,
  initialTeam,
}: {
  code: string;
  initialTeam: TeamResponse;
}) {
  const router = useRouter();
  const { teams } = useTeams();
  const [selectedTeamId, setSelectedTeamId] = useState(initialTeam.id);
  const [isRedeeming, setIsRedeeming] = useState(false);
  const [redemptionError, setRedemptionError] = useState<string>();
  const promoState = usePromoLookup(code);
  const selectedTeam = useMemo(
    () =>
      teams?.find((team) => team.id === selectedTeamId) ??
      (initialTeam.id === selectedTeamId ? initialTeam : null),
    [initialTeam, selectedTeamId, teams],
  );
  const {
    subscription,
    isLoading: isSubscriptionLoading,
    hasError: hasSubscriptionError,
  } = useTeamOrbSubscription(selectedTeam?.id);
  const { plans } = useListPlans(selectedTeam?.id);
  const creditsResult = useListCredits(selectedTeam?.id ?? null);
  const starterPlan = plans?.find(
    (plan) => plan.planType === "CONVEX_STARTER_PLUS",
  );
  const teamPlan: TeamPlan = hasSubscriptionError
    ? "error"
    : isSubscriptionLoading || subscription === undefined
      ? "loading"
      : subscription === null
        ? "free"
        : "paid";
  const starterUpgradeUrl =
    selectedTeam && starterPlan
      ? `/t/${encodeURIComponent(selectedTeam.slug)}/settings/billing?upgradePlan=${encodeURIComponent(starterPlan.id)}`
      : undefined;

  const close = () => {
    const query = { ...router.query };
    delete query.promoCode;
    void router.replace({ pathname: router.pathname, query }, undefined, {
      shallow: true,
    });
  };

  const redeem = async () => {
    if (
      !selectedTeam ||
      promoState.status !== "success" ||
      teamPlan !== "paid"
    ) {
      return;
    }
    setRedemptionError(undefined);
    setIsRedeeming(true);
    try {
      const response = await fetch("/api/redeem-promo", {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ code, teamId: selectedTeam.id }),
      });
      const result = (await response.json()) as {
        error?: string;
        credit_amount?: number;
      };
      if (!response.ok) {
        setRedemptionError(result.error ?? "Unable to redeem this promo code.");
        return;
      }

      toast(
        "success",
        `Added ${formatUsd(result.credit_amount ?? promoState.promo.creditAmount)} in credits to ${selectedTeam.name}`,
      );
      try {
        await creditsResult.refreshCredits();
      } catch {
        toast(
          "error",
          "Credits were added, but the credit list could not be refreshed.",
        );
      }
      await router.push({
        pathname: "/t/[team]/settings/billing",
        query: { team: selectedTeam.slug },
      });
    } catch {
      setRedemptionError("Unable to redeem the promo code. Please try again.");
    } finally {
      setIsRedeeming(false);
    }
  };

  return (
    <PromoCodeModal
      promoState={promoState}
      teams={teams}
      selectedTeam={selectedTeam}
      teamPlan={teamPlan}
      starterUpgradeUrl={starterUpgradeUrl}
      isRedeeming={isRedeeming}
      redemptionError={redemptionError}
      onSelectTeam={(team) => {
        setSelectedTeamId(team.id);
        setRedemptionError(undefined);
      }}
      onRedeem={redeem}
      onClose={close}
    />
  );
}

function usePromoLookup(code: string): PromoLookupState {
  const [state, setState] = useState<PromoLookupState>({ status: "loading" });

  useEffect(() => {
    const controller = new AbortController();
    setState({ status: "loading" });
    void (async () => {
      try {
        const response = await fetch("/api/lookup-promo", {
          method: "POST",
          headers: { "Content-Type": "application/json" },
          body: JSON.stringify({ code }),
          signal: controller.signal,
        });
        const result = (await response.json()) as {
          error?: string;
          code?: string;
          description?: string;
          credit_amount?: number;
          expiration_time?: number;
          credit_validity_days?: number;
        };
        if (!response.ok) {
          setState({
            status: "error",
            error: result.error ?? "Unable to load this promo code.",
          });
          return;
        }
        if (
          result.code === undefined ||
          result.description === undefined ||
          result.credit_amount === undefined ||
          result.expiration_time === undefined ||
          result.credit_validity_days === undefined
        ) {
          setState({
            status: "error",
            error: "The promo service returned an invalid response.",
          });
          return;
        }
        setState({
          status: "success",
          promo: {
            code: result.code,
            description: result.description,
            creditAmount: result.credit_amount,
            expirationTime: result.expiration_time,
            creditValidityDays: result.credit_validity_days,
          },
        });
      } catch (error) {
        if (error instanceof DOMException && error.name === "AbortError") {
          return;
        }
        setState({
          status: "error",
          error: "Unable to load this promo code. Please try again.",
        });
      }
    })();
    return () => controller.abort();
  }, [code]);

  return state;
}
