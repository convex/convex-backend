import { useState } from "react";
import { useListInvoices, useTeamOrbSubscription } from "api/billing";
import { Callout } from "@ui/Callout";
import { Loading } from "@ui/Loading";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { OpenInVercel } from "components/OpenInVercel";
import { TeamResponse } from "generatedApi";
import startCase from "lodash/startCase";
import { Invoices } from "./Invoices";

// Sized to fill a typical viewport, since the table takes the page's full
// height.
const INVOICES_PAGE_SIZE = 25;

export function TeamInvoices({ team }: { team: TeamResponse }) {
  return (
    <>
      <div className="flex flex-col gap-1">
        <h2>Invoices</h2>
        <span className="text-sm text-content-secondary">
          Preview or download your upcoming and past invoices.
        </span>
      </div>
      {team.managedBy === "vercel" ? (
        <Callout variant="upsell">
          <div className="flex w-full items-center justify-between gap-4">
            <div>
              This team is managed by {startCase(team.managedBy)}. You can find
              your invoices in the {startCase(team.managedBy)} dashboard.
            </div>
            <OpenInVercel team={team} />
          </div>
        </Callout>
      ) : (
        <TeamInvoicesList team={team} />
      )}
    </>
  );
}

function TeamInvoicesList({ team }: { team: TeamResponse }) {
  const [limit, setLimit] = useState(INVOICES_PAGE_SIZE);
  const invoicesResult = useListInvoices(team.id, limit);
  const { subscription } = useTeamOrbSubscription(team.id);

  if (invoicesResult.status === "loading") {
    return <Loading className="h-60 w-full" fullHeight={false} />;
  }
  if (invoicesResult.status === "denied") {
    return (
      <NoPermissionMessage
        message="You do not have permission to view invoices for this team."
        missingPermission={invoicesResult.deniedAction}
      />
    );
  }
  const { invoices, hasMore, isRefreshing } = invoicesResult.data;
  return (
    <Invoices
      invoices={invoices}
      invoicingThreshold={subscription?.invoicingThreshold}
      onShowMore={
        hasMore ? () => setLimit(limit + INVOICES_PAGE_SIZE) : undefined
      }
      isLoadingMore={isRefreshing}
    />
  );
}
