import { useState } from "react";
import {
  DotsVerticalIcon,
  ExclamationTriangleIcon,
  ExternalLinkIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import startCase from "lodash/startCase";
import { Callout } from "@ui/Callout";
import { Link } from "@ui/Link";
import { Checkbox } from "@ui/Checkbox";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Loading } from "@ui/Loading";
import { cn } from "@ui/cn";
import { HelpTooltip } from "@ui/HelpTooltip";
import { Menu, MenuItem } from "@ui/Menu";
import type { SsoPortalIntent, TeamResponse } from "generatedApi";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import {
  useDisableSSO,
  useGenerateSSOConfigurationLink,
  useGetSSO,
  useTeamEntitlements,
  useUpdateSSO,
} from "api/teams";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { SSO_RESOURCE } from "lib/permissions";
import { ProviderIcon } from "./ProviderIcon";
import {
  ConfigurationRow,
  EmptyStateRow,
  LoadErrorState,
  SHEET_ROW,
  SettingsSheet,
} from "./SettingsSheet";
import { ConnectionStatusBadge } from "./StatusBadge";
import { connectionProvider, useReportUnmappedProviders } from "./providers";

const SSO_DOCS_LINK = (
  <Link href="https://docs.convex.dev/team-management/sso" target="_blank">
    Learn more about SSO
  </Link>
);

export function SingleSignOnSheet({ team }: { team: TeamResponse }) {
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canView = useHasCustomRolePermission(
    team.id,
    "sso:view",
    SSO_RESOURCE,
    true,
  );
  // Opening the WorkOS portal goes through
  // `generate_sso_configuration_link`, which the server gates on `sso:enable`
  // whichever intent it is given.
  const canConfigureCustom = useHasCustomRolePermission(
    team.id,
    "sso:enable",
    SSO_RESOURCE,
    false,
  );
  const canUpdateCustom = useHasCustomRolePermission(
    team.id,
    "sso:update",
    SSO_RESOURCE,
    false,
  );
  const canDisableCustom = useHasCustomRolePermission(
    team.id,
    "sso:disable",
    SSO_RESOURCE,
    false,
  );
  const canConfigure = isTeamAdmin || canConfigureCustom === true;
  const canUpdate = isTeamAdmin || canUpdateCustom === true;
  const canDisable = isTeamAdmin || canDisableCustom === true;

  const entitlements = useTeamEntitlements(team.id);
  const ssoEntitled = entitlements?.ssoEnabled ?? false;
  const { data: sso, error: ssoError } = useGetSSO(team.id, {
    isPaused: canView !== true,
  });
  const generateSSOConfigurationLink = useGenerateSSOConfigurationLink(team.id);
  const disableSSO = useDisableSSO(team.id);
  const updateSSO = useUpdateSSO(team.id);

  const [isGeneratingLink, setIsGeneratingLink] = useState(false);
  const [showDisableConfirmation, setShowDisableConfirmation] = useState(false);
  const [disableError, setDisableError] = useState<string>();
  const [isDisabling, setIsDisabling] = useState(false);
  // The value the member just toggled the checkbox to, pending confirmation.
  // `null` means the checkbox reflects the server's value.
  const [pendingRequireSsoLogin, setPendingRequireSsoLogin] = useState<
    boolean | null
  >(null);
  const [isSavingRequireSsoLogin, setIsSavingRequireSsoLogin] = useState(false);
  const [requireSsoLoginError, setRequireSsoLoginError] = useState<string>();

  const connections = (sso?.connections ?? []).map((connection) => ({
    ...connection,
    provider: connectionProvider(connection.connectionType),
  }));
  useReportUnmappedProviders(connections.map((c) => c.provider));
  const configured = connections.length > 0;
  const isLoadingSso = sso === undefined;
  // SWR hands back the last good response while it revalidates, so a failed
  // background refresh leaves the configuration on screen; only a failure with
  // nothing to fall back on takes the sheet over.
  const failedToLoad = isLoadingSso && ssoError !== undefined;
  const hasVerifiedDomain = (sso?.domains ?? []).some(
    (d) => d.state === "verified" || d.state === "legacyVerified",
  );
  const hasActiveConnection = connections.some((c) => c.active);
  const requireSsoLogin = sso?.requireSsoLogin ?? false;

  const openPortal = async (intent: SsoPortalIntent) => {
    setIsGeneratingLink(true);
    try {
      const result = await generateSSOConfigurationLink({ intent });
      if (result?.link) {
        window.open(result.link, "_blank");
      }
    } finally {
      setIsGeneratingLink(false);
    }
  };

  if (canView === false) {
    return (
      <SettingsSheet
        title="Single sign-on"
        description={
          <>
            Configure an identity provider for your team members to use as a
            login method. {SSO_DOCS_LINK}.
          </>
        }
      >
        <div className="px-6 py-10">
          <NoPermissionMessage
            message="You do not have permission to view SSO settings."
            missingPermission="sso:view"
          />
        </div>
      </SettingsSheet>
    );
  }

  const configureTip = !canConfigure
    ? permissionDeniedTip(
        "You do not have permission to configure SSO.",
        "sso:enable",
      )
    : !ssoEntitled
      ? // The button carries the upsell, so an unentitled team reads why it
        // cannot configure SSO where it would have configured it.
        team.managedBy === "vercel"
        ? `SSO is not available for teams managed by ${startCase(team.managedBy)}.`
        : "SSO is not available on your plan. Upgrade your plan to use SSO."
      : !hasVerifiedDomain
        ? "Verify a domain before configuring an identity provider."
        : undefined;
  const canOpenPortal =
    canConfigure && ssoEntitled && hasVerifiedDomain && !isGeneratingLink;

  const requireSsoLoginTip = !canUpdate
    ? permissionDeniedTip(
        "You do not have permission to change SSO settings.",
        "sso:update",
      )
    : !ssoEntitled
      ? "SSO is not available on your plan."
      : !hasActiveConnection
        ? "Your identity provider connection must be active before requiring SSO."
        : "Require that team members log in with SSO to access the team.";

  const requireSsoLoginRow = (
    <div className="flex w-fit items-center gap-2 text-sm">
      <label className="ml-px flex items-center gap-2">
        <Checkbox
          checked={pendingRequireSsoLogin ?? requireSsoLogin}
          disabled={!canUpdate || !ssoEntitled || !hasActiveConnection}
          onChange={() => setPendingRequireSsoLogin(!requireSsoLogin)}
        />
        Require SSO to access team
      </label>
      {/* One trigger for both the explanation and the reason the checkbox is
          unavailable; wrapping the label itself would nest the checkbox inside
          the tooltip's button. */}
      <HelpTooltip tipSide="right">{requireSsoLoginTip}</HelpTooltip>
    </div>
  );

  return (
    <SettingsSheet
      title="Single sign-on"
      testId="sso-sheet"
      description={
        configured ? (
          <>Manage your SSO identity provider configuration. {SSO_DOCS_LINK}.</>
        ) : (
          <>
            Configure an identity provider for your team members to use as a
            login method. {SSO_DOCS_LINK}.
          </>
        )
      }
    >
      {failedToLoad ? (
        <LoadErrorState
          title="Error fetching SSO configuration"
          description="An error occurred while fetching your SSO configuration. Please try again later."
        />
      ) : isLoadingSso ? (
        <Loading fullHeight={false} className="m-3 h-10" />
      ) : configured ? (
        <>
          <div className="flex w-full flex-col divide-y">
            {connections.map((connection) => (
              <ConfigurationRow
                key={connection.id}
                title={connection.provider.label}
                icon={<ProviderIcon provider={connection.provider} />}
                badge={<ConnectionStatusBadge connection={connection} />}
                menu={
                  <Menu
                    placement="bottom-end"
                    buttonProps={{
                      variant: "neutral",
                      size: "xs",
                      icon: <DotsVerticalIcon />,
                      "aria-label": `${connection.provider.label} options`,
                    }}
                  >
                    <MenuItem
                      disabled={!canOpenPortal}
                      tip={configureTip}
                      action={() => {
                        void openPortal("sso");
                      }}
                    >
                      Manage
                    </MenuItem>
                    {connection.connectionType.endsWith("SAML") ? (
                      <MenuItem
                        disabled={!canOpenPortal}
                        tip={configureTip}
                        action={() => {
                          void openPortal("certificateRenewal");
                        }}
                      >
                        Renew certificate
                      </MenuItem>
                    ) : null}
                    <MenuItem
                      variant="danger"
                      disabled={!canDisable}
                      tip={
                        canDisable
                          ? undefined
                          : permissionDeniedTip(
                              "You do not have permission to disable SSO.",
                              "sso:disable",
                            )
                      }
                      action={() => setShowDisableConfirmation(true)}
                    >
                      Disable Single sign-on
                    </MenuItem>
                  </Menu>
                }
              />
            ))}
          </div>
          {/* Requiring SSO is a team-level setting, so it sits under the
              connections rather than repeating on each one. */}
          <div className={cn(SHEET_ROW, "border-t")}>{requireSsoLoginRow}</div>
        </>
      ) : (
        <EmptyStateRow
          message="Single sign-on has not been configured."
          action={
            <Button
              size="sm"
              className="w-fit shrink-0"
              icon={<ExternalLinkIcon />}
              loading={isGeneratingLink}
              disabled={!canOpenPortal}
              tip={configureTip}
              onClick={() => openPortal("sso")}
            >
              Configure
            </Button>
          }
        />
      )}

      {pendingRequireSsoLogin !== null && (
        <ConfirmationDialog
          onClose={() => {
            if (isSavingRequireSsoLogin) {
              return;
            }
            // Cancelling puts the checkbox back where the server has it.
            setPendingRequireSsoLogin(null);
            setRequireSsoLoginError(undefined);
          }}
          onConfirm={async () => {
            setRequireSsoLoginError(undefined);
            setIsSavingRequireSsoLogin(true);
            try {
              await updateSSO({ requireSsoLogin: pendingRequireSsoLogin });
              setPendingRequireSsoLogin(null);
            } catch (e: any) {
              setRequireSsoLoginError(
                typeof e?.message === "string"
                  ? e.message
                  : "Failed to update SSO settings.",
              );
              throw e;
            } finally {
              setIsSavingRequireSsoLogin(false);
            }
          }}
          confirmText="Save changes"
          variant="primary"
          dialogTitle={
            pendingRequireSsoLogin
              ? "Require SSO to access team"
              : "Stop requiring SSO to access team"
          }
          dialogBody={
            pendingRequireSsoLogin ? (
              <div className="flex flex-col gap-2">
                <p>
                  Team members will have to log in with SSO to access the team.
                </p>
                <Callout
                  variant="instructions"
                  className="mt-0 flex items-center"
                >
                  <ExclamationTriangleIcon className="mr-1 text-content-warning" />
                  Test your SSO configuration thoroughly before enabling this
                  setting.
                </Callout>
              </div>
            ) : (
              "Team members will no longer have to log in with SSO to access the team."
            )
          }
          disableConfirm={isSavingRequireSsoLogin}
          validationText="CONFIRM SSO SETTINGS"
          error={requireSsoLoginError}
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
              await disableSSO();
              setShowDisableConfirmation(false);
            } catch (e: any) {
              setDisableError(e.message);
              throw e;
            } finally {
              setIsDisabling(false);
            }
          }}
          confirmText="Disable"
          variant="danger"
          dialogTitle="Disable Single sign-on"
          dialogBody="This removes your identity provider connection, and team members will no longer be able to log in with SSO. Your domains and Directory Sync configuration are not affected."
          error={disableError}
          validationText="DISABLE SINGLE SIGN-ON"
        />
      )}
    </SettingsSheet>
  );
}
