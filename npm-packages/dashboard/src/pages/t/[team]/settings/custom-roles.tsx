import { CustomRoles } from "components/teamSettings/CustomRoles";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";

export { getServerSideProps } from "lib/ssr";

function CustomRolesPage() {
  return (
    <TeamSettingsLayout
      page="custom-roles"
      Component={CustomRoles}
      title="Custom Roles"
    />
  );
}

export default withAuthenticatedPage(CustomRolesPage);
