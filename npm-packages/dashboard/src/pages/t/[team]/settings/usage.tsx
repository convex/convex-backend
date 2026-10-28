import { TeamUsage } from "components/billing/TeamUsage";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

export function TeamUsagePage() {
  return (
    <TeamSettingsLayout page="usage" Component={TeamUsage} title="Usage" />
  );
}

export default withAuthenticatedPage(TeamUsagePage);
