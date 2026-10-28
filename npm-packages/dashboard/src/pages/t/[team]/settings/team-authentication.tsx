import { TeamSSO } from "components/teamSettings/teamAuthentication/TeamSSO";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

export function TeamAuthenticationPage() {
  return (
    <TeamSettingsLayout
      page="team-authentication"
      Component={TeamSSO}
      title="Team Authentication"
      // The review subpage bounds its table against the page's height and
      // scrolls it, so the page needs a height that does not grow with it.
      fillHeight
    />
  );
}

export default withAuthenticatedPage(TeamAuthenticationPage);
