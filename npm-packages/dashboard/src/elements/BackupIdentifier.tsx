import { BackupResponse, useGetCloudBackup } from "api/backups";
import { useDeploymentByName } from "api/deployments";
import { useCurrentTeam } from "api/teams";
import { useProjectById } from "api/projects";
import { Loading } from "@ui/Loading";

export function BackupIdentifier({ backup }: { backup: BackupResponse }) {
  const team = useCurrentTeam();
  const deployment = useDeploymentByName(backup.sourceDeploymentName);
  const { project } = useProjectById(deployment?.projectId);
  return (
    <span className="text-xs text-content-secondary">
      {team?.slug}-{project?.slug}-{deployment?.name}-
      {new Date(backup.requestedTime).getTime()}
    </span>
  );
}

export function CloudImport({
  sourceCloudBackupId,
}: {
  sourceCloudBackupId: number;
}) {
  const backup = useGetCloudBackup(sourceCloudBackupId);
  const ident = backup ? (
    <BackupIdentifier backup={backup} />
  ) : (
    <Loading fullHeight={false} className="inline-block h-3 w-80" />
  );
  return <span>restored from backup: {ident}</span>;
}
