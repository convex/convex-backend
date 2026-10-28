import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { toast } from "@common/lib/utils";
import { Sheet } from "@ui/Sheet";
import { LocalDevCallout } from "@common/elements/LocalDevCallout";
import { Callout } from "@ui/Callout";
import { TextInput } from "@ui/TextInput";
import { Popover } from "@ui/Popover";
import {
  useDisablePeriodicBackup,
  useGetPeriodicBackupConfig,
  useConfigurePeriodicBackup,
} from "api/backups";
import { useCurrentProject } from "api/projects";
import { useContext, useId, useMemo, useState } from "react";
import { PermissionsContext } from "@common/lib/deploymentContext";
import { PlatformDeploymentResponse } from "@convex-dev/platform/managementApi";
import { TeamResponse, TeamEntitlementsResponse } from "generatedApi";
import { Link } from "@ui/Link";
import { useQuery } from "convex/react";
import udfs from "@common/udfs";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { deploymentResource } from "lib/permissions";
import { ChevronDownIcon, InfoCircledIcon } from "@radix-ui/react-icons";
import { Combobox } from "@ui/Combobox";
import { BackupList } from "./BackupList";
import { BackupRestoreStatus } from "./BackupRestoreStatus";
import {
  BackupStorageSelector,
  EstimatedSize,
  backupPricingTip,
  estimatedBackupSize,
  useBackupStorageUsage,
} from "./BackupStorageSelector";

export function Backups({
  team,
  deployment,
  entitlements,
}: {
  team: TeamResponse;
  deployment: PlatformDeploymentResponse;
  entitlements: TeamEntitlementsResponse;
}) {
  const project = useCurrentProject();

  const existingExport = useQuery(udfs.latestExport.default);
  const periodicBackupsEnabled = entitlements.periodicBackupsEnabled ?? false;
  const maxCloudBackups = entitlements.maxCloudBackups ?? 2;

  const hasAdminPermissions = useHasProjectAdminPermissions(
    deployment.projectId,
  );

  // Built-in role semantics for write actions: admin can always act,
  // developer is allowed on non-prod deployments. Custom-role members are
  // gated per-action by their explicit grants.
  const builtinAllowed = deployment.deploymentType !== "prod";
  const resource =
    project && deployment.kind === "cloud"
      ? deploymentResource(project, {
          id: deployment.id,
          deploymentType: deployment.deploymentType,
          creator: deployment.creator ?? null,
        })
      : undefined;
  const { useIsOperationAllowed } = useContext(PermissionsContext);
  const canCreate = useIsOperationAllowed("CreateBackups");
  const canImport = useIsOperationAllowed("ImportBackups");
  const canDelete = useIsOperationAllowed("DeleteBackups");
  // Periodic backup config/disable are management-plane actions (not
  // data-plane DeploymentOps), so they go through useHasCustomRolePermission
  // directly instead of useIsOperationAllowed.
  const canConfigurePeriodicCustom = useHasCustomRolePermission(
    team.id,
    "deployment:backups:configurePeriodic",
    resource,
    builtinAllowed,
  );
  const canDisablePeriodicCustom = useHasCustomRolePermission(
    team.id,
    "deployment:backups:disablePeriodic",
    resource,
    builtinAllowed,
  );
  const canConfigurePeriodic =
    hasAdminPermissions || canConfigurePeriodicCustom === true;
  const canDisablePeriodic =
    hasAdminPermissions || canDisablePeriodicCustom === true;

  const isDedicated =
    deployment.kind === "cloud" && deployment.class.startsWith("d");

  return (
    <div className="flex h-full flex-col gap-4">
      <div className="flex flex-wrap items-center justify-between gap-4">
        <h3 className="min-w-fit">Backup & Restore</h3>
        <span className="text-sm">
          Use this page to automatically or manually backup and restore your
          deployment data.{" "}
          <Link href="https://docs.convex.dev/database/backup-restore">
            Learn more about backups
          </Link>
        </span>
      </div>
      {isDedicated && <PhysicalBackupsSection deployment={deployment} />}
      {isDedicated && (
        <h4 className="flex min-w-fit items-center gap-1.5">
          Zip backups
          <Tooltip tip="Restoring a zip backup into a dedicated deployment isn't supported. Contact the Convex team to restore from a physical backup.">
            <InfoCircledIcon className="size-4 text-content-secondary" />
          </Tooltip>
        </h4>
      )}
      <div className="scrollbar flex grow flex-col gap-4 overflow-auto pt-1 pl-1">
        <Sheet className="flex h-fit w-full shrink-0 flex-col items-start gap-6">
          <div className="flex w-full flex-col gap-4">
            <h4 className="text-content-primary">Backup Schedule</h4>
            {periodicBackupsEnabled ? (
              <AutomaticBackupSelector
                teamId={team.id}
                deployment={deployment}
                canConfigurePeriodic={canConfigurePeriodic}
                canDisablePeriodic={canDisablePeriodic}
              />
            ) : (
              <Tooltip
                tip="Automatic backups are only available on the Pro plan."
                className="flex items-center gap-3"
              >
                <div className="flex items-center gap-2 text-sm">
                  <span>Backup</span>
                  <span className="w-fit cursor-not-allowed rounded-md border bg-background-secondary px-3 py-1.5 text-content-secondary">
                    Never
                  </span>
                </div>
                <span
                  className="h-fit rounded-sm bg-util-accent px-1.5 py-1 text-xs font-semibold tracking-wider text-white uppercase"
                  title="Only available on the Pro plan"
                >
                  Pro
                </span>
              </Tooltip>
            )}
          </div>
          <BackupProCallouts
            team={team}
            periodicBackupsEnabled={periodicBackupsEnabled}
            maxCloudBackups={maxCloudBackups}
          />
        </Sheet>

        <div className="flex flex-col gap-4 pb-8">
          {existingExport &&
            existingExport._creationTime < new Date("2024-11-15").getTime() &&
            existingExport.state === "completed" &&
            Date.now() <
              Number(existingExport.expiration_ts / BigInt(1000000)) && (
              <Callout>
                <div>
                  Looking for your last Snapshot Export? You can now use Cloud
                  Backups to backup and restore your deployment data. Download
                  your last snapshot{" "}
                  <Link
                    href={`/t/${team.slug}/${project?.slug}/${deployment.name}/settings/snapshots`}
                  >
                    here
                  </Link>
                  .
                </div>
              </Callout>
            )}
          <BackupRestoreStatus deployment={deployment} />

          <Sheet padding={false} className="min-h-72">
            <BackupList
              teamId={team.id}
              targetDeployment={deployment}
              canCreate={canCreate}
              canImport={canImport}
              canDelete={canDelete}
              maxCloudBackups={maxCloudBackups}
            />
          </Sheet>
        </div>
      </div>
    </div>
  );
}

function PhysicalBackupsSection({
  deployment,
}: {
  deployment: PlatformDeploymentResponse;
}) {
  if (deployment.kind !== "cloud") return null;
  return (
    <div className="flex flex-col gap-2">
      <h4 className="min-w-fit">Physical backups</h4>
      <Callout>
        <span>
          {deployment.class.toUpperCase()} deployments include physical backups.
          Physical backups are faster and cheaper due to the ability to rely on
          dedicated database hardware. They are produced every 24 hours and are
          retained for 7 days. To restore from a physical backup, contact the
          Convex team.{" "}
          <Link href="https://docs.convex.dev/database/backup-restore">
            Learn more about backups
          </Link>
        </span>
      </Callout>
    </div>
  );
}

function BackupProCallouts({
  team,
  periodicBackupsEnabled,
  maxCloudBackups,
}: {
  team: TeamResponse;
  periodicBackupsEnabled: boolean;
  maxCloudBackups: number;
}) {
  return (
    <>
      {!periodicBackupsEnabled && (
        <LocalDevCallout
          className="mt-6 flex-col"
          tipText="Tip: Run this to enable automatic backups locally:"
          command={`just big-brain-tool-dev entitlement grant add --team-entitlement periodic_backups_enabled --team-id ${team?.id} --reason "local" true --for-real`}
        />
      )}
      {maxCloudBackups <= 2 && (
        <LocalDevCallout
          className="mt-6 flex-col"
          tipText="Tip: Run this to increase the backup limit locally:"
          command={`just big-brain-tool-dev entitlement grant add --team-entitlement max_cloud_backups --team-id ${team?.id} --reason "local" 50 --for-real`}
        />
      )}
    </>
  );
}

export function AutomaticBackupSelector({
  teamId,
  deployment,
  canConfigurePeriodic,
  canDisablePeriodic,
}: {
  teamId: number;
  deployment: PlatformDeploymentResponse;
  canConfigurePeriodic: boolean;
  canDisablePeriodic: boolean;
}) {
  const deploymentId = deployment.kind === "cloud" ? deployment.id : undefined;
  const periodicBackup = useGetPeriodicBackupConfig(deploymentId);
  const configurePeriodicBackup = useConfigurePeriodicBackup(deploymentId);
  const usage = useBackupStorageUsage(teamId, deployment);

  // Set only while a toggle is in flight; otherwise the checkbox reads
  // straight from the config. Mirroring the config into state instead paints
  // one frame with the stale value, before the effect doing the copy runs.
  const [pendingIncludeStorage, setPendingIncludeStorage] = useState<boolean>();
  const includeStorage =
    pendingIncludeStorage ?? periodicBackup?.includeStorage ?? false;

  const handleIncludeStorageChange = async (newValue: boolean) => {
    if (!periodicBackup) {
      return;
    }
    setPendingIncludeStorage(newValue);

    try {
      await configurePeriodicBackup({
        ...periodicBackup,
        includeStorage: newValue,
      });
      toast(
        "success",
        `Updated automatic backups to include ${newValue ? "file storage" : "tables only"}.`,
      );
    } catch {
      // `useBBMutation` has already toasted the failure, and dropping the
      // pending value below falls back to the unchanged server value.
    } finally {
      // The mutation revalidates the config before it resolves, so the server
      // value is already current by the time the pending one drops.
      setPendingIncludeStorage(undefined);
    }
  };

  return (
    <div className="flex w-full flex-col gap-2">
      <div className="mb-1 flex items-center gap-2 text-sm">
        <span>Backup</span>
        <BackupScheduleSelector
          cronspec={periodicBackup?.cronspec ?? null}
          deployment={deployment}
          loading={periodicBackup === undefined}
          canConfigurePeriodic={canConfigurePeriodic}
          canDisablePeriodic={canDisablePeriodic}
        />
        {periodicBackup && (
          <span className="flex items-center gap-1.5">
            <EstimatedSize
              bytes={estimatedBackupSize(usage, includeStorage)}
              error={usage.error}
            />
            <Tooltip tip={backupPricingTip} aria-label="Backup usage pricing">
              <InfoCircledIcon className="size-3.5 text-content-secondary" />
            </Tooltip>
          </span>
        )}
      </div>
      {periodicBackup && (
        <>
          <BackupStorageSelector
            teamId={teamId}
            deployment={deployment}
            includeStorage={includeStorage}
            setIncludeStorage={handleIncludeStorageChange}
            disabled={!canConfigurePeriodic}
            isSubmitting={pendingIncludeStorage !== undefined}
            showEstimatedSize={false}
            usage={usage}
          />
          <div className="flex flex-wrap items-baseline gap-x-1">
            <TimestampDistance
              prefix="Next backup "
              date={new Date(periodicBackup.nextRun)}
            />
            <span className="text-xs text-content-secondary">
              ({new Date(periodicBackup.nextRun).toLocaleString()}{" "}
              {localTimezoneName()})
            </span>
          </div>
        </>
      )}
    </div>
  );
}

export function BackupScheduleSelector({
  cronspec,
  deployment,
  disabled = false,
  loading = false,
  canConfigurePeriodic = true,
  canDisablePeriodic = true,
}: {
  cronspec: string | null;
  deployment: PlatformDeploymentResponse;
  disabled?: boolean;
  loading?: boolean;
  canConfigurePeriodic?: boolean;
  canDisablePeriodic?: boolean;
}) {
  const defaultCronspec = useMemo(() => {
    if (cronspec !== null) {
      return cronspec;
    }
    // Stagger newly enabled backups instead of concentrating them at a fixed
    // default time.
    const randomHour = Math.floor(Math.random() * 24);
    const randomMinute = Math.floor(Math.random() * 60);
    return `${randomMinute} ${randomHour} * * *`;
  }, [cronspec]);
  const parts = defaultCronspec.split(" ");
  const [minutesUtc, hoursUtc, , , dayOfWeekPart = "*"] = parts;
  const isWeekly = dayOfWeekPart !== "*";
  const dayOfWeekNum = isWeekly ? Number(dayOfWeekPart) : null;
  const defaultDayOfWeek = useMemo(
    () =>
      // We randomize the default day of week to spread out the backups
      // of users that don’t specify a custom time
      Math.floor(Math.random() * 7),
    [],
  );
  const date = new Date();
  date.setUTCHours(+hoursUtc, +minutesUtc);
  const canOpen =
    cronspec === null
      ? canConfigurePeriodic
      : canConfigurePeriodic || canDisablePeriodic;

  return (
    <Popover
      button={
        <Button
          variant="neutral"
          className="relative min-w-24 pr-10 pl-3 font-normal"
          disabled={disabled || loading || !canOpen}
          loading={loading}
          tip={
            !canOpen
              ? "You do not have permission to change the automatic backup settings."
              : undefined
          }
        >
          <span className="flex flex-col truncate">
            {cronspec === null ? "Never" : isWeekly ? "Weekly" : "Daily"}
          </span>
          <span className="pointer-events-none absolute inset-y-0 right-0 flex items-center pr-2">
            <ChevronDownIcon
              className="size-5 text-content-tertiary"
              aria-hidden="true"
            />
          </span>
        </Button>
      }
      openButtonClassName="*:bg-background-tertiary"
    >
      {({ close }) => (
        <BackupScheduleSelectorInner
          defaultValue={date}
          defaultPeriodicity={
            cronspec === null ? "never" : isWeekly ? "weekly" : "daily"
          }
          defaultDayOfWeek={dayOfWeekNum ?? defaultDayOfWeek}
          onClose={close}
          deployment={deployment}
          canConfigurePeriodic={canConfigurePeriodic}
          canDisablePeriodic={canDisablePeriodic}
        />
      )}
    </Popover>
  );
}

export function BackupScheduleSelectorInner({
  defaultValue,
  defaultPeriodicity,
  defaultDayOfWeek,
  onClose,
  deployment,
  canConfigurePeriodic = true,
  canDisablePeriodic = true,
}: {
  defaultValue: Date;
  defaultPeriodicity: "never" | "daily" | "weekly";
  defaultDayOfWeek: number;
  onClose: () => void;
  deployment: PlatformDeploymentResponse;
  canConfigurePeriodic?: boolean;
  canDisablePeriodic?: boolean;
}) {
  const deploymentId = deployment.kind === "cloud" ? deployment.id : undefined;
  const configurePeriodicBackup = useConfigurePeriodicBackup(deploymentId);
  const disablePeriodicBackup = useDisablePeriodicBackup(deploymentId);

  const initialValue = `${defaultValue.getHours().toString().padStart(2, "0")}:${defaultValue.getMinutes().toString().padStart(2, "0")}`;
  const [value, setValue] = useState(initialValue);

  const id = useId();

  const [isSubmitting, setIsSubmitting] = useState(false);

  const [periodicity, setPeriodicity] = useState(defaultPeriodicity);
  const [selectedDow, setSelectedDow] = useState(defaultDayOfWeek);
  const isUnchanged =
    periodicity === defaultPeriodicity &&
    (periodicity === "never" ||
      (value === initialValue &&
        (periodicity !== "weekly" || defaultDayOfWeek === selectedDow)));
  const missingPermission =
    periodicity === "never" ? !canDisablePeriodic : !canConfigurePeriodic;

  return (
    <form
      className="flex min-w-72 flex-col items-end gap-3"
      onSubmit={async (e) => {
        e.preventDefault();

        setIsSubmitting(true);
        try {
          if (periodicity === "never") {
            await disablePeriodicBackup();
          } else {
            const [newHoursLocal, newMinutesLocal] = value.split(":");
            const nowLocal = new Date();
            nowLocal.setHours(+newHoursLocal, +newMinutesLocal);
            const enablingAutomaticBackups = defaultPeriodicity === "never";
            await configurePeriodicBackup({
              cronspec:
                periodicity === "daily"
                  ? `${nowLocal.getUTCMinutes()} ${nowLocal.getUTCHours()} * * *`
                  : `${nowLocal.getUTCMinutes()} ${nowLocal.getUTCHours()} * * ${selectedDow}`,
              ...(periodicity === "weekly"
                ? { expirationDeltaSecs: 14 * 24 * 60 * 60 }
                : {}),
              ...(enablingAutomaticBackups ? { includeStorage: false } : {}),
            });
            toast("success", "Your backup schedule was modified.");
          }
        } finally {
          setIsSubmitting(false);
        }

        onClose();
      }}
    >
      <div className="flex w-full flex-col gap-3">
        <div className="flex items-center gap-2 text-sm">
          <label className="flex items-center gap-1">
            <input
              type="radio"
              value="never"
              checked={periodicity === "never"}
              onChange={() => setPeriodicity("never")}
              disabled={defaultPeriodicity !== "never" && !canDisablePeriodic}
            />
            Never
          </label>
          <label className="flex items-center gap-1">
            <input
              type="radio"
              value="daily"
              checked={periodicity === "daily"}
              onChange={() => setPeriodicity("daily")}
              disabled={!canConfigurePeriodic}
            />
            Daily
          </label>
          <label className="flex items-center gap-1">
            <input
              type="radio"
              value="weekly"
              checked={periodicity === "weekly"}
              onChange={() => setPeriodicity("weekly")}
              disabled={!canConfigurePeriodic}
            />
            Weekly
          </label>
        </div>
        {periodicity === "weekly" && (
          <Combobox
            label="Day of week"
            buttonClasses="w-full"
            optionsWidth="full"
            options={[
              { value: 0, label: "Sunday" },
              { value: 1, label: "Monday" },
              { value: 2, label: "Tuesday" },
              { value: 3, label: "Wednesday" },
              { value: 4, label: "Thursday" },
              { value: 5, label: "Friday" },
              { value: 6, label: "Saturday" },
            ]}
            selectedOption={selectedDow}
            setSelectedOption={(dow) => dow !== null && setSelectedDow(dow)}
            disableSearch
            disabled={!canConfigurePeriodic}
          />
        )}
        {periodicity !== "never" && (
          <TextInput
            id={id}
            type="time"
            label={`Time (${localTimezoneName()})`}
            value={value}
            onChange={(e) => setValue(e.target.value)}
            required
            disabled={!canConfigurePeriodic}
          />
        )}
        {defaultPeriodicity === "never" && periodicity !== "never" && (
          <p className="max-w-72 text-xs text-content-secondary">
            {backupPricingTip}
          </p>
        )}
        <div className="flex w-full justify-end">
          <Button
            type="submit"
            disabled={isUnchanged || missingPermission}
            loading={isSubmitting}
          >
            Change
          </Button>
        </div>
      </div>
    </form>
  );
}

function localTimezoneName(): string {
  return new Intl.DateTimeFormat(undefined, {
    timeZoneName: "short",
  })
    .formatToParts(new Date())
    .find((part) => part.type === "timeZoneName")!.value;
}
