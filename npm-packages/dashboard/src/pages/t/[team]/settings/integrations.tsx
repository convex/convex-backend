import { TeamIntegrations } from "components/teamSettings/integrations/TeamIntegrations";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

export function TeamIntegrationsPage() {
  return (
    <TeamSettingsLayout
      page="integrations"
      Component={TeamIntegrations}
      title="Integrations"
    />
  );
}

export default withAuthenticatedPage(TeamIntegrationsPage);
