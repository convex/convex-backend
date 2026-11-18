import { ExternalLinkIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Callout } from "@ui/Callout";
import { Combobox } from "@ui/Combobox";
import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { Link } from "@ui/Link";
import { Loading } from "@ui/Loading";
import { MenuItem } from "@ui/Menu";
import { Modal } from "@ui/Modal";
import { CopyTextButton } from "@common/elements/CopyTextButton";
import { integrationToLogo } from "@common/lib/integrationHelpers";
import {
  useAvailableWorkOSTeamEmails,
  useDisconnectWorkOSTeam,
  useProvisionWorkOSTeam,
  useTeamWorkOSIntegration,
  useWorkOSTeamHealth,
} from "api/workos";
import { WorkOsTeamIntegrationResponse } from "generatedApi";
import { useState } from "react";
import { useTeamMembers } from "api/teams";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { CatalogCard, CatalogMenu, CatalogStatus } from "./CatalogCard";

const headerClass =
  "px-3 py-2 text-left text-xs font-medium text-content-secondary";
const cellClass = "px-3 py-2 align-middle text-sm";

export const WORKOS_CATALOG_ENTRY = {
  name: "WorkOS",
  category: "Authentication",
  description:
    "Authenticate users with AuthKit, from one WorkOS workspace for the whole team.",
};

const MANAGE_PERMISSION_TIP =
  "Only team admins and members with integration access can manage WorkOS.";

type Health = ReturnType<typeof useWorkOSTeamHealth>;

function healthCode(health: Health) {
  // The spec declares no error body, but Big Brain errors carry a `code`.
  return (health.error as { code?: string } | undefined)?.code;
}

/**
 * The team's WorkOS workspace. Its AuthKit environments belong to individual
 * deployments, but the workspace, and creating or disconnecting it, is
 * team-wide.
 */
export function WorkOSTeamIntegrationCard({
  teamId,
  canConnect,
  canDisconnect,
}: {
  teamId: number;
  canConnect: boolean;
  canDisconnect: boolean;
}) {
  const integration = useTeamWorkOSIntegration(teamId.toString());
  const workspace = integration?.teamAssociation ?? null;
  const health = useWorkOSTeamHealth(workspace ? teamId.toString() : undefined);
  const disconnect = useDisconnectWorkOSTeam(teamId.toString());
  const [showDetails, setShowDetails] = useState(false);
  const [showDisconnectDialog, setShowDisconnectDialog] = useState(false);

  const code = healthCode(health);
  const workspaceMissing =
    code === "WorkOSTeamNotFound" || code === "WorkOSTeamDeleted";

  // The card only reports on a workspace that exists.
  const footer = workspace ? (
    <CatalogStatus
      dot={
        workspaceMissing
          ? "bg-util-error"
          : health.error
            ? "bg-util-warning"
            : health.data
              ? "bg-util-success"
              : undefined
      }
    >
      {workspaceMissing ? "Workspace missing in WorkOS" : "Connected to "}
      {!workspaceMissing && (
        <span className="font-semibold text-content-primary">
          {workspace.workosTeamName}
        </span>
      )}
    </CatalogStatus>
  ) : undefined;

  return (
    <>
      <CatalogCard
        logo={integrationToLogo("workos").logo}
        name={WORKOS_CATALOG_ENTRY.name}
        category={WORKOS_CATALOG_ENTRY.category}
        description={WORKOS_CATALOG_ENTRY.description}
        menu={
          <CatalogMenu name={WORKOS_CATALOG_ENTRY.name}>
            {workspace ? (
              <MenuItem action={() => setShowDetails(true)}>
                View details
              </MenuItem>
            ) : (
              <MenuItem
                action={() => setShowDetails(true)}
                disabled={!canConnect || integration === undefined}
                tip={canConnect ? undefined : MANAGE_PERMISSION_TIP}
                tipSide="left"
              >
                Install WorkOS
              </MenuItem>
            )}
            {workspace ? (
              <MenuItem
                variant="danger"
                action={() => setShowDisconnectDialog(true)}
                disabled={!canDisconnect}
                tip={canDisconnect ? undefined : MANAGE_PERMISSION_TIP}
                tipSide="left"
              >
                Disconnect
              </MenuItem>
            ) : null}
          </CatalogMenu>
        }
        footer={footer}
      />
      {showDetails && (
        <Modal
          size="md"
          title={
            <span className="flex items-center gap-2">
              {integrationToLogo("workos", true).logo}
              {workspace ? "WorkOS" : "Install WorkOS"}
            </span>
          }
          onClose={() => setShowDetails(false)}
        >
          {integration === undefined ? (
            <Loading className="h-24" fullHeight={false} />
          ) : workspace ? (
            <WorkOSWorkspaceDetails
              teamId={teamId}
              integration={integration}
              workspace={workspace}
              health={health}
            />
          ) : (
            <CreateWorkOSWorkspace teamId={teamId} canConnect={canConnect} />
          )}
        </Modal>
      )}
      {showDisconnectDialog && workspace && (
        <ConfirmationDialog
          onClose={() => setShowDisconnectDialog(false)}
          onConfirm={async () => {
            await disconnect({ teamId });
          }}
          confirmText="Disconnect"
          dialogTitle="Disconnect WorkOS"
          dialogBody={
            <>
              Convex can no longer create AuthKit environments in the{" "}
              {workspace.workosTeamName} workspace. Existing environments keep
              working.
            </>
          }
        />
      )}
    </>
  );
}

function WorkOSWorkspaceDetails({
  teamId,
  integration,
  workspace,
  health,
}: {
  teamId: number;
  integration: WorkOsTeamIntegrationResponse;
  workspace: NonNullable<WorkOsTeamIntegrationResponse["teamAssociation"]>;
  health: Health;
}) {
  const code = healthCode(health);
  const productionState = health.data?.teamInfo?.productionState;
  // The response names the creator but not their member ID; their email is
  // the member email it was copied from.
  const creator = useTeamMembers(teamId)?.find(
    (member) => member.email === workspace.creatorEmail,
  );

  return (
    <div className="flex flex-col gap-4 text-sm">
      <p className="text-content-secondary">
        Convex creates an AuthKit environment in this workspace for each
        deployment that uses AuthKit.
      </p>
      {code === "WorkOSTeamNotFound" || code === "WorkOSTeamDeleted" ? (
        <Callout variant="error" className="mt-0">
          {code === "WorkOSTeamDeleted"
            ? "This workspace was deleted in WorkOS."
            : "This workspace can't be found in WorkOS."}{" "}
          Disconnect it to create another.
        </Callout>
      ) : code === "WorkOSAPIUnavailable" ? (
        <Callout className="mt-0">
          WorkOS is unavailable, so the workspace can&apos;t be checked right
          now.
        </Callout>
      ) : (
        productionState &&
        productionState !== "active" && (
          <Callout className="mt-0">
            <span>
              <Link
                href="https://dashboard.workos.com/settings/billing"
                target="_blank"
                rel="noopener noreferrer"
                className="text-content-primary underline"
              >
                Add a payment method in WorkOS
              </Link>{" "}
              to create production AuthKit environments.
            </span>
          </Callout>
        )
      )}
      <dl className="grid grid-cols-[auto_1fr] gap-x-4 gap-y-2">
        <dt className="text-content-secondary">Workspace</dt>
        <dd>{workspace.workosTeamName}</dd>
        <dt className="text-content-secondary">Workspace ID</dt>
        <dd>
          <CopyTextButton
            text={workspace.workosTeamId}
            className="font-mono text-xs"
          />
        </dd>
        <dt className="text-content-secondary">Admin</dt>
        <dd>{workspace.adminEmail}</dd>
        <dt className="text-content-secondary">Created by</dt>
        <dd>
          {creator ? (
            <TeamMemberLink
              memberId={creator.id}
              name={creator.name || creator.email}
            />
          ) : (
            (workspace.creatorName ?? workspace.creatorEmail)
          )}
        </dd>
      </dl>
      <div className="flex flex-col gap-2">
        <h5>AuthKit environments</h5>
        {integration.environments.length === 0 ? (
          <p className="text-content-secondary">
            No deployment has an AuthKit environment yet.
          </p>
        ) : (
          <div
            className="scrollbar max-h-64 overflow-y-auto rounded-md border"
            // eslint-disable-next-line jsx-a11y/no-noninteractive-tabindex -- https://dequeuniversity.com/rules/axe/4.11/scrollable-region-focusable?application=axeAPI
            tabIndex={0}
          >
            <table className="w-full border-collapse">
              <thead className="sticky top-0 bg-background-secondary">
                <tr className="border-b">
                  <th className={headerClass}>Deployment</th>
                  <th className={headerClass}>WorkOS environment</th>
                </tr>
              </thead>
              <tbody className="divide-y">
                {integration.environments.map((environment) => (
                  <tr key={environment.workosEnvironmentId}>
                    <td className={cellClass}>
                      <Link
                        href={`/d/${environment.deploymentName}`}
                        target="_blank"
                        rel="noopener noreferrer"
                        className="inline-flex items-center gap-1 font-mono"
                      >
                        {environment.deploymentName}
                        <ExternalLinkIcon className="size-3 shrink-0" />
                      </Link>
                    </td>
                    <td className={cellClass}>
                      {environment.workosEnvironmentName}
                    </td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        )}
      </div>
      <Link
        href="https://dashboard.workos.com"
        target="_blank"
        rel="noopener noreferrer"
      >
        Open the WorkOS dashboard
      </Link>
    </div>
  );
}

function CreateWorkOSWorkspace({
  teamId,
  canConnect,
}: {
  teamId: number;
  canConnect: boolean;
}) {
  const emails = useAvailableWorkOSTeamEmails();
  const provision = useProvisionWorkOSTeam(teamId.toString());
  const [chosenEmail, setChosenEmail] = useState<string>();
  const [isCreating, setIsCreating] = useState(false);
  const [error, setError] = useState<string | null>(null);

  if (emails === undefined) {
    return <Loading className="h-24" fullHeight={false} />;
  }
  const email = chosenEmail ?? emails.availableEmails[0] ?? "";

  const create = async () => {
    setIsCreating(true);
    setError(null);
    try {
      await provision({ teamId, email });
    } catch (e) {
      const { code, message } = (e ?? {}) as {
        code?: string;
        message?: string;
      };
      setError(
        code === "WorkosAccountAlreadyExistsWithThisEmail" ||
          message?.includes("already")
          ? `A WorkOS workspace already exists for ${email}. Choose another email.`
          : "Couldn't create the WorkOS workspace. Try again.",
      );
    } finally {
      setIsCreating(false);
    }
  };

  return (
    <div className="flex flex-col gap-4 text-sm">
      <p className="text-content-secondary">
        Create a WorkOS workspace for this team. Convex then creates an{" "}
        <Link href="https://docs.convex.dev/auth/authkit" target="_blank">
          AuthKit
        </Link>{" "}
        environment in it for each deployment that uses AuthKit.
      </p>
      {error && (
        <Callout variant="error" className="mt-0">
          {error}
        </Callout>
      )}
      {emails.availableEmails.length === 0 ? (
        <p className="text-content-secondary">
          Every verified email on your account already administers a WorkOS
          workspace.{" "}
          <Link href="/profile" target="_blank">
            Add and verify another email
          </Link>{" "}
          to create one.
        </p>
      ) : (
        <>
          <div className="flex flex-col gap-1">
            <Combobox
              label="Admin email"
              labelHidden={false}
              options={emails.availableEmails.map((value) => ({
                value,
                label: value,
              }))}
              selectedOption={email}
              setSelectedOption={(value) => setChosenEmail(value ?? undefined)}
              disableSearch={emails.availableEmails.length <= 5}
              allowCustomValue={false}
              buttonClasses="w-full bg-inherit"
            />
            <p className="text-xs text-content-secondary">
              WorkOS invites this email as the workspace admin. To use another,{" "}
              <Link href="/profile" target="_blank">
                verify it in your profile
              </Link>
              .
            </p>
          </div>
          <div>
            <Button
              onClick={create}
              disabled={!canConnect || !email}
              loading={isCreating}
              tip={canConnect ? undefined : MANAGE_PERMISSION_TIP}
            >
              Create workspace
            </Button>
          </div>
        </>
      )}
    </div>
  );
}
