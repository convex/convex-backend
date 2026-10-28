import { Loading } from "@ui/Loading";
import { toast } from "@common/lib/utils";
import { AuditLog } from "components/teamSettings/AuditLog";
import { useCurrentTeam, useTeamEntitlements } from "api/teams";
import { TeamSettingsLayout } from "layouts/TeamSettingsLayout";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { useRouter } from "next/router";

export { getServerSideProps } from "lib/ssr";

export function AuditLogPage() {
  const team = useCurrentTeam();
  const auditLogRetentionDays = useTeamEntitlements(
    team?.id,
  )?.auditLogRetentionDays;
  const router = useRouter();

  if (auditLogRetentionDays === undefined) {
    return <Loading />;
  }
  if (auditLogRetentionDays === 0) {
    toast("info", "The audit log is only available on the Pro plan.", "upsell");
    void router.push(`/t/${router.query.team}/settings/billing`);
    return null;
  }

  return (
    <TeamSettingsLayout
      page="audit-log"
      Component={AuditLog}
      title="Audit Log"
    />
  );
}

export default withAuthenticatedPage(AuditLogPage);
