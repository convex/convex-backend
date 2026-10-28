import { CreditResponse } from "generatedApi";
import { formatUtcDate } from "@common/lib/format";
import { formatUsd } from "@common/lib/utils";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Tooltip } from "@ui/Tooltip";
import { Donut } from "@ui/Donut";

export function PrepaidCredits({ credits }: { credits: CreditResponse[] }) {
  if (credits.length === 0) {
    return null;
  }
  return (
    <>
      <div className="flex flex-col gap-4">
        <div className="flex items-center gap-1">
          <h4>Credits</h4>
          <HelpTooltip tipSide="right">
            Credits are applied to your invoices before your payment method is
            charged. They're spent soonest-expiring first, and any balance left
            over when a credit expires is forfeited.
          </HelpTooltip>
        </div>
        <div className="flex flex-col gap-3">
          {credits.map((credit) => (
            <Credit key={credit.id} credit={credit} />
          ))}
        </div>
      </div>
      <hr />
    </>
  );
}

function Credit({ credit }: { credit: CreditResponse }) {
  const { balance, initialBalance } = credit;
  const used = Math.max(0, initialBalance - balance);
  const usedLabel = `${formatUsd(used)} of ${formatUsd(initialBalance)} used`;
  const percentUsed = initialBalance > 0 ? (100 * used) / initialBalance : 0;

  return (
    <div className="flex items-center gap-3">
      <Tooltip
        side="bottom"
        tip={`You've used ${percentUsed.toFixed(1)}% of this credit.`}
        className="flex items-center"
      >
        <Donut current={used} max={initialBalance} />
      </Tooltip>
      <div className="flex min-w-0 grow flex-col gap-0.5">
        <CreditTitle credit={credit} />
        <div className="flex flex-col text-sm">
          <span>{usedLabel}</span>
          <span className="text-content-secondary">
            <CreditStatus credit={credit} />
          </span>
        </div>
      </div>
    </div>
  );
}

// Credit from a plan allocation is named after what it was allocated for;
// credit granted by hand carries a note instead. A credit can have both, and a
// hand-granted one that wasn't annotated has neither.
function CreditTitle({ credit }: { credit: CreditResponse }) {
  const { itemName, description } = credit;
  if (!itemName && !description) {
    return null;
  }
  return (
    <div className="flex flex-wrap items-baseline gap-x-2 text-sm">
      {itemName && <span className="font-medium">{itemName}</span>}
      {description && (
        // Standing alone, the note is the credit's name and reads as one.
        <span className={itemName ? "text-content-secondary" : "font-medium"}>
          {description}
        </span>
      )}
    </div>
  );
}

function CreditStatus({ credit }: { credit: CreditResponse }) {
  const { expiryDate } = credit;

  if (expiryDate !== null && expiryDate !== undefined) {
    return <>Expires on {formatUtcDate(new Date(expiryDate))}</>;
  }
  return <>Never expires</>;
}
