import { useState } from "react";
import { useRouter } from "next/router";
import { DotsVerticalIcon, ExternalLinkIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Link } from "@ui/Link";
import { Loading } from "@ui/Loading";
import { Menu, MenuItem } from "@ui/Menu";
import { Spinner } from "@ui/Spinner";
import { cn } from "@ui/cn";
import type { TeamResponse } from "generatedApi";
import {
  useDisableDirectorySync,
  useGenerateDirectorySyncConfigurationLink,
  useGetDirectorySync,
} from "api/directorySync";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { useGetSSO, useTeamEntitlements } from "api/teams";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import {
  DIRECTORY_SYNC_RESOURCE,
  MEMBER_RESOURCE,
  SSO_RESOURCE,
} from "lib/permissions";
import { ConnectDirectoryDialog } from "./ConnectDirectoryDialog";
import { DirectoryGroupsSheet } from "./DirectoryGroupsSheet";
import { ProviderIcon } from "./ProviderIcon";
import {
  ConfigurationRow,
  EmptyStateRow,
  LoadErrorState,
  SettingsSheet,
  SHEET_ROW,
} from "./SettingsSheet";
import { DirectoryStatusBadge } from "./StatusBadge";
import { directoryProvider, useReportUnmappedProviders } from "./providers";

const DIRECTORY_SYNC_DOCS_LINK = (
  <Link
    href="https://docs.convex.dev/team-management/directory-sync"
    target="_blank"
  >
    Learn more about Directory Sync
  </Link>
);

const NOT_CONFIGURED_DESCRIPTION = (
  <>
    Configure Directory Sync to automatically provision and deprovision team
    members. {DIRECTORY_SYNC_DOCS_LINK}.
  </>
);

export const INITIAL_SYNC_IN_PROGRESS =
  "Initial sync in progress. It may take up to an hour to finish. Once the sync has finished you will be able to review and enable your configuration here.";

export function DirectorySyncSheet({ team }: { team: TeamResponse }) {
  const router = useRouter();
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canView = useHasCustomRolePermission(
    team.id,
    "directorySync:view",
    DIRECTORY_SYNC_RESOURCE,
    true,
  );
  const canEnableCustom = useHasCustomRolePermission(
    team.id,
    "directorySync:enable",
    DIRECTORY_SYNC_RESOURCE,
    false,
  );
  const canDisableCustom = useHasCustomRolePermission(
    team.id,
    "directorySync:disable",
    DIRECTORY_SYNC_RESOURCE,
    false,
  );
  const canUpdateMappingCustom = useHasCustomRolePermission(
    team.id,
    "directorySync:updateGroupMapping",
    DIRECTORY_SYNC_RESOURCE,
    false,
  );
  // The staged roster names each member and the role they hold, so the
  // endpoint behind the review subpage asks for `member:view` as well.
  const canViewMembers = useHasCustomRolePermission(
    team.id,
    "member:view",
    MEMBER_RESOURCE,
    true,
  );
  const canEnable = isTeamAdmin || canEnableCustom === true;
  const canDisable = isTeamAdmin || canDisableCustom === true;
  const canUpdateMapping = isTeamAdmin || canUpdateMappingCustom === true;

  const entitlements = useTeamEntitlements(team.id);
  const directorySyncEntitled = entitlements?.directorySyncEnabled ?? false;
  const customRolesEnabled = entitlements?.customRolesEnabled ?? false;
  const { data: directorySync, error: directorySyncError } =
    useGetDirectorySync(team.id, {
      isPaused: canView !== true,
    });
  // A directory can only provision members on a verified domain, so the
  // domains sheet gates this one too.
  const canViewSSO = useHasCustomRolePermission(
    team.id,
    "sso:view",
    SSO_RESOURCE,
    true,
  );
  const { data: sso, error: ssoError } = useGetSSO(team.id, {
    isPaused: canViewSSO !== true,
  });
  const generateLink = useGenerateDirectorySyncConfigurationLink(team.id);
  const disableDirectorySync = useDisableDirectorySync(team.id);

  const provider = directoryProvider(directorySync?.directory?.type);
  useReportUnmappedProviders([provider]);

  const [isGeneratingLink, setIsGeneratingLink] = useState(false);
  const [showConnectDialog, setShowConnectDialog] = useState(false);
  const [showDisableConfirmation, setShowDisableConfirmation] = useState(false);
  const [isDisabling, setIsDisabling] = useState(false);
  const [disableError, setDisableError] = useState<string>();

  // The roster is a subpage of team authentication rather than a dialog, so
  // it is reached by navigating within the page.
  const goToReview = () => {
    void router.push(
      {
        pathname: "/t/[team]/settings/team-authentication",
        query: { team: team.slug, review: "1" },
      },
      undefined,
      { shallow: true },
    );
  };

  if (canView === false) {
    return (
      <SettingsSheet
        title="Directory sync"
        description={NOT_CONFIGURED_DESCRIPTION}
      >
        <div className="px-6 py-10">
          <NoPermissionMessage
            message="You do not have permission to view Directory Sync settings."
            missingPermission="directorySync:view"
          />
        </div>
      </SettingsSheet>
    );
  }

  const reported = directorySync?.directory ?? undefined;
  const directory = reported?.state === "deleting" ? undefined : reported;
  // The query is paused until the member's roles resolve `canView`, and SWR
  // only reports `isLoading` on the mount that starts a request — a query that
  // unpauses later reports `false` with nothing to show. Absent data is what
  // says this is still loading; otherwise the sheet renders its not-configured
  // state, Configure button and all, before the skeleton appears.
  const isLoadingDirectory = directorySync === undefined;
  const hasVerifiedDomain = (sso?.domains ?? []).some(
    (d) => d.state === "verified" || d.state === "legacyVerified",
  );
  // SWR hands back the last good response while it revalidates, so a failed
  // background refresh leaves the configuration on screen; only a failure with
  // nothing to fall back on takes the sheet over. The SSO query counts too: it
  // decides whether a verified domain exists, and without it the sheet would
  // offer to configure a directory that cannot be configured.
  const failedToLoad =
    (isLoadingDirectory && directorySyncError !== undefined) ||
    (sso === undefined && ssoError !== undefined);

  const directoryTitle = provider?.label ?? directory?.name ?? "Directory";

  const openPortal = async () => {
    setIsGeneratingLink(true);
    try {
      const result = await generateLink();
      if (result?.link) {
        window.open(result.link, "_blank");
      }
    } finally {
      setIsGeneratingLink(false);
    }
  };

  const configureTip = !canEnable
    ? permissionDeniedTip(
        "You do not have permission to configure Directory Sync.",
        "directorySync:enable",
      )
    : !directorySyncEntitled
      ? // The button carries the upsell, so an unentitled team reads why it
        // cannot configure directory sync where it would have configured it.
        "Directory Sync is not available on your plan. Upgrade your plan to use Directory Sync."
      : !hasVerifiedDomain
        ? "Verify a domain before configuring Directory Sync."
        : undefined;
  const canOpenPortal =
    canEnable &&
    directorySyncEntitled &&
    hasVerifiedDomain &&
    !isGeneratingLink;

  const managementEnabled = directorySync?.enabled ?? false;
  // Until the roster is enabled the directory provisions nobody, so tearing it
  // down is a deletion rather than a change to how members are managed.
  const disableTitle = managementEnabled
    ? "Disable Directory Sync"
    : "Delete directory";

  const awaitingInitialSync =
    directory?.linked === true && directorySync?.mirrored === false;
  const rosterTip =
    canViewMembers === false
      ? permissionDeniedTip(
          "You do not have permission to view team members.",
          "member:view",
        )
      : undefined;
  const reviewTip =
    rosterTip ??
    (!canEnable
      ? permissionDeniedTip(
          "You do not have permission to enable directory sync.",
          "directorySync:enable",
        )
      : !directorySyncEntitled
        ? "Directory Sync is not available on your plan."
        : undefined);

  return (
    // The groups sheet reads as part of the directory's configuration, so the
    // two sit closer together than the page's sections do.
    <div className="flex flex-col gap-6">
      <SettingsSheet
        title="Directory sync"
        testId="directory-sync-sheet"
        description={
          directory ? (
            <>
              Manage your Directory Sync configuration.{" "}
              {DIRECTORY_SYNC_DOCS_LINK}.
            </>
          ) : (
            NOT_CONFIGURED_DESCRIPTION
          )
        }
      >
        {failedToLoad ? (
          <LoadErrorState
            title="Error fetching Directory Sync configuration"
            description="An error occurred while fetching your Directory Sync configuration. Please try again later."
          />
        ) : isLoadingDirectory ? (
          <Loading fullHeight={false} className="m-3 h-10" />
        ) : directory ? (
          <>
            <ConfigurationRow
              title={directoryTitle}
              icon={provider && <ProviderIcon provider={provider} />}
              badge={<DirectoryStatusBadge state={directory.state} />}
              menu={
                <Menu
                  placement="bottom-end"
                  buttonProps={{
                    variant: "neutral",
                    size: "xs",
                    icon: <DotsVerticalIcon />,
                    "aria-label": `${directoryTitle} options`,
                  }}
                >
                  {managementEnabled ? (
                    <MenuItem
                      disabled={rosterTip !== undefined}
                      tip={rosterTip}
                      action={goToReview}
                    >
                      View pending members
                    </MenuItem>
                  ) : null}
                  <MenuItem
                    disabled={!canOpenPortal}
                    tip={configureTip}
                    action={() => {
                      void openPortal();
                    }}
                  >
                    Manage
                  </MenuItem>
                  <MenuItem
                    variant="danger"
                    disabled={!canDisable}
                    tip={
                      canDisable
                        ? undefined
                        : permissionDeniedTip(
                            managementEnabled
                              ? "You do not have permission to disable Directory Sync."
                              : "You do not have permission to delete this directory.",
                            "directorySync:disable",
                          )
                    }
                    action={() => setShowDisableConfirmation(true)}
                  >
                    {disableTitle}
                  </MenuItem>
                </Menu>
              }
            />
            {!directory.linked ? (
              <div
                className={cn(
                  SHEET_ROW,
                  "flex items-center gap-4 border-t text-sm",
                )}
              >
                <span className="text-content-secondary">
                  This directory is not linked yet. Check its connection with
                  your identity provider to finish setting up directory sync.
                </span>
                <div className="ml-auto">
                  <Button
                    size="xs"
                    icon={<ExternalLinkIcon />}
                    disabled={!canOpenPortal}
                    tip={configureTip}
                    onClick={() => setShowConnectDialog(true)}
                  >
                    Check connection
                  </Button>
                </div>
              </div>
            ) : !managementEnabled ? (
              <div
                className={cn(
                  SHEET_ROW,
                  "flex items-center gap-4 border-t text-sm",
                  // The accent marks a row waiting on the member; the initial
                  // sync waits on the identity provider instead.
                  !awaitingInitialSync &&
                    "bg-util-accent/10 dark:bg-util-accent/30",
                )}
              >
                {awaitingInitialSync ? (
                  <div className="flex items-center gap-2">
                    <Spinner className="ml-0 shrink-0" />
                    <span className="text-content-secondary">
                      {INITIAL_SYNC_IN_PROGRESS}
                    </span>
                  </div>
                ) : (
                  <>
                    <span className="text-content-primary">
                      Directory sync is not yet enabled. Please review directory
                      role mappings to enable automatic provisioning.
                    </span>
                    <div className="ml-auto">
                      <Button
                        size="xs"
                        disabled={reviewTip !== undefined}
                        tip={reviewTip}
                        onClick={goToReview}
                      >
                        Review
                      </Button>
                    </div>
                  </>
                )}
              </div>
            ) : null}
          </>
        ) : (
          <EmptyStateRow
            message="Directory Sync has not been configured."
            action={
              <Button
                size="sm"
                className="w-fit shrink-0"
                icon={<ExternalLinkIcon />}
                disabled={!canOpenPortal}
                tip={configureTip}
                onClick={() => setShowConnectDialog(true)}
              >
                Configure
              </Button>
            }
          />
        )}

        {showConnectDialog && (
          <ConnectDirectoryDialog
            teamId={team.id}
            onClose={() => setShowConnectDialog(false)}
          />
        )}

        {showDisableConfirmation && (
          <ConfirmationDialog
            onClose={() => {
              if (isDisabling) {
                return;
              }
              setShowDisableConfirmation(false);
              setDisableError(undefined);
            }}
            onConfirm={async () => {
              setIsDisabling(true);
              try {
                await disableDirectorySync();
                setShowDisableConfirmation(false);
              } catch (e: any) {
                setDisableError(e.message);
                throw e;
              } finally {
                setIsDisabling(false);
              }
            }}
            confirmText={managementEnabled ? "Disable" : "Delete"}
            variant="danger"
            dialogTitle={disableTitle}
            dialogBody={
              <div className="flex flex-col gap-3 text-sm">
                <p>
                  {managementEnabled
                    ? "This disconnects your directory, and team members will no longer be provisioned or deprovisioned by your identity provider. Members already on the team keep their access."
                    : "This disconnects your directory from your identity provider. Directory Sync never started provisioning, so your team members are unaffected."}
                </p>
                {/* True of both wordings: the teardown deletes the directory
                    itself, and the groups and mappings keyed off it go with
                    it. */}
                <p>
                  Your directory's groups and their role mappings are discarded
                  along with it. Setting Directory Sync up again means
                  connecting the directory and mapping every group to a role
                  from scratch.
                </p>
              </div>
            }
            error={disableError}
            validationText={
              managementEnabled ? "DISABLE DIRECTORY SYNC" : "DELETE DIRECTORY"
            }
          />
        )}
      </SettingsSheet>
      {directory?.linked && (
        <DirectoryGroupsSheet
          team={team}
          canEdit={canUpdateMapping}
          customRolesEnabled={customRolesEnabled}
          awaitingInitialSync={awaitingInitialSync}
        />
      )}
    </div>
  );
}
