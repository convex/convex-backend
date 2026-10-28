import classNames from "classnames";
import { useCurrentTeam } from "api/teams";
import { useTeamOrbSubscription } from "api/billing";
import { Link } from "@ui/Link";
import { useIsCurrentMemberTeamAdmin } from "api/roles";

export function UpdateBillingAddressBanner() {
  const team = useCurrentTeam();
  return (
    <div
      className={classNames(
        "flex flex-wrap items-center px-2 py-1 gap-1 border-b h-24 sm:h-12 overflow-x-hidden",
        "bg-background-warning text-content-warning text-xs",
      )}
    >
      Your subscription is missing a full billing address. Please{" "}
      <Link href={`/t/${team?.slug}/settings/billing#billingAddress`} passHref>
        update it on the Billing page
      </Link>{" "}
      to avoid issues processing future invoices.
    </div>
  );
}

export function useShowUpdateBillingAddressBanner() {
  const team = useCurrentTeam();
  const { subscription: orbSubscription } = useTeamOrbSubscription(team?.id);
  // Only team admins can update the billing address, so the banner is not
  // actionable for anyone else.
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  if (!isTeamAdmin) {
    return false;
  }
  // Team billing is managed by Vercel
  if (team?.managedBy === "vercel") {
    return false;
  }
  // Team does not have a paid subscription
  if (!orbSubscription) {
    return false;
  }
  // Team's subscription is not active
  if (orbSubscription.endDate && orbSubscription.endDate < Date.now()) {
    return false;
  }

  // Team's subscription already has a full billing address
  // Note: Not including postal code or city here because not all countries have these
  if (
    orbSubscription.billingAddress?.line1 &&
    orbSubscription.billingAddress?.country
  ) {
    return false;
  }

  // Even if the team has a partial billing address, we should still show the
  // banner to try to collect the full address. We migrated whatever address
  // information we could get from the credit card stored in Stripe, but in many
  // cases this was only a zip code and country.
  return true;
}
