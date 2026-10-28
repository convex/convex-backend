import { HelpTooltip } from "@ui/HelpTooltip";
import { formatUsd } from "@common/lib/utils";

export function SubscriptionCredits({
  accountBalance,
}: {
  // The customer's account balance as a decimal string (e.g. "25.00"). This is
  // a credit applied to future invoices and is never negative.
  accountBalance?: string | null;
}) {
  // The account balance is a credit that is automatically applied to future
  // invoices; it is never negative for our customers.
  const parsedBalance =
    typeof accountBalance === "string" ? parseFloat(accountBalance) : NaN;
  const balance =
    Number.isFinite(parsedBalance) && parsedBalance > 0 ? parsedBalance : null;

  // Only surface the balance when there's a (positive) credit to report.
  if (balance === null) {
    return null;
  }

  return (
    <div className="flex items-center gap-1 text-sm">
      Account balance:
      <span className="font-semibold">{formatUsd(balance)}</span>
      <HelpTooltip tipSide="top">
        Your account balance is a credit that's automatically applied to your
        upcoming invoices. It builds up when you cancel a subscription or add or
        remove seats mid-cycle.
      </HelpTooltip>
    </div>
  );
}
