import { TeamMembers } from "components/teamSettings/TeamMembers";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

export function TeamMembersPage() {
  return (
    <TeamSettingsLayout
      page="members"
      Component={TeamMembers}
      title="Members"
    />
  );
}

export default withAuthenticatedPage(TeamMembersPage);
