import { TeamInvoices } from "components/billing/TeamInvoices";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

export function TeamInvoicesPage() {
  return (
    <TeamSettingsLayout
      page="invoices"
      Component={TeamInvoices}
      title="Invoices"
      // The invoices table scrolls within the page's height rather than
      // growing past it.
      fillHeight
    />
  );
}

export default withAuthenticatedPage(TeamInvoicesPage);
