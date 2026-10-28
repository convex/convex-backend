import { TeamSettings } from "components/teamSettings/TeamSettings";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

export function TeamSettingsPage() {
  return (
    <TeamSettingsLayout
      page="general"
      Component={TeamSettings}
      title="Team Settings"
    />
  );
}

export default withAuthenticatedPage(TeamSettingsPage);
