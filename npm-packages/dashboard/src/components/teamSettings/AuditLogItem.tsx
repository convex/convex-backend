import {
  Disclosure,
  DisclosurePanel,
  DisclosureButton,
} from "@headlessui/react";
import {
  ChevronUpIcon,
  ChevronDownIcon,
  ArrowRightIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { ReadonlyCode } from "@common/elements/ReadonlyCode";
import { TimestampDistance } from "@common/elements/TimestampDistance";
import { stringifyValue } from "@common/lib/stringifyValue";
import {
  PlatformDeploymentResponse,
  AuditLogEventResponse,
} from "@convex-dev/platform/managementApi";
import { TeamResponse, MemberResponse } from "generatedApi";
import { AuditLogAction } from "api/auditLog";
import { captureMessage } from "@sentry/nextjs";
import startCase from "lodash/startCase";
import { Link } from "@ui/Link";
import { useDeploymentByName } from "api/deployments";
import { BackupIdentifier } from "elements/BackupIdentifier";
import { TeamMemberLink } from "elements/TeamMemberLink";
import { Tooltip } from "@ui/Tooltip";
import { HelpTooltip } from "@ui/HelpTooltip";
import { formatUsd } from "@common/lib/utils";
import { useProjectById } from "api/projects";

// TODO: Figure out how to get typing on metadata in
// big brain
type AuditLogEntryMetadata = {
  noun?: string;
  previous?: Record<string, any> | null;
  current?: Record<string, any> | null;
};

export function AuditLogItem({
  entry,
  team,
  memberId,
  members,
}: {
  entry: AuditLogEventResponse;
  team: TeamResponse;
  memberId: number | null;
  members: MemberResponse[];
}) {
  return (
    <Disclosure>
      {({ open }) => (
        <div
          className="border-b px-6 py-2 last:border-b-0"
          data-testid="audit-log-item"
        >
          <div className="grid grid-cols-[9fr_3fr]">
            <span className="text-sm">
              <AuditLogItemActor
                entry={entry}
                memberId={memberId}
                members={members}
              />{" "}
              <EntryAction
                action={entry.action}
                metadata={entry.metadata as AuditLogEntryMetadata}
                team={team}
                members={members}
              />
            </span>
            <span className="ml-auto flex gap-1">
              <TimestampDistance date={new Date(entry.createTime)} />
              <Tooltip tip="View entry" side="left" asChild>
                <DisclosureButton
                  as={Button}
                  inline
                  variant="neutral"
                  size="xs"
                  aria-label={open ? "Hide details" : "Show details"}
                >
                  {open ? <ChevronUpIcon /> : <ChevronDownIcon />}
                </DisclosureButton>
              </Tooltip>
            </span>
          </div>
          <DisclosurePanel>
            <ReadonlyCode
              height={{
                type: "content",
              }}
              disableLineNumbers
              code={stringifyValue(
                JSON.stringify(entry, undefined, 2),
                true,
              ).slice(1, -1)}
              path={`${entry.createTime}`}
            />
          </DisclosurePanel>
        </div>
      )}
    </Disclosure>
  );
}

function EntryAction({
  action,
  metadata,
  team,
  members,
}: {
  action: AuditLogAction;
  metadata: AuditLogEntryMetadata;
  team: TeamResponse;
  members: MemberResponse[];
}) {
  switch (action) {
    case "project:create":
    case "project:update":
    case "project:delete":
      return (
        <ProjectEntryAction team={team} action={action} metadata={metadata} />
      );
    case "project:receive":
      return (
        <span>
          transferred project{" "}
          <ProjectLink
            projectId={metadata.current?.id}
            metadata={metadata}
            team={team}
          />{" "}
          to this team.
        </span>
      );
    case "project:transfer":
      return (
        <span>
          transferred project{" "}
          <ProjectLink
            projectId={metadata.previous?.id}
            metadata={metadata}
            team={team}
          />{" "}
          to another team.
        </span>
      );
    case "billing:contact:update":
      return <span>updated the billing contact</span>;
    case "billing:address:update":
      return <span>updated the billing address</span>;
    case "billing:paymentMethod:update":
      return <span>updated the payment method</span>;
    case "member:remove":
      if (!metadata.previous?.email) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          removed{" "}
          <span className="font-semibold">{metadata.previous.email}</span> from
          the team
        </span>
      );
    case "member:invite":
      if (!metadata.current?.email) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          invited{" "}
          <span className="font-semibold">{metadata.current.email}</span> to the
          team
        </span>
      );
    case "member:cancelInvitation":
      if (!metadata.previous?.email) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          canceled the team invitation for{" "}
          <span className="font-semibold">{metadata.previous.email}</span>
        </span>
      );
    case "member:updateRole":
      if (
        !metadata.current?.role ||
        !metadata.current?.id ||
        (!metadata.current?.name && !metadata.current?.email)
      ) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          changed{" "}
          <TeamMemberLink
            memberId={metadata.current.id}
            name={metadata.current.name || metadata.current.email}
          />
          's role to{" "}
          <span className="font-semibold">
            {startCase(metadata.current.role)}
          </span>
        </span>
      );
    case "project:updateMemberRole":
      if (
        metadata.current &&
        metadata.current.project_id &&
        metadata.current.role &&
        metadata.current.member_id
      ) {
        return (
          <ProjectRoleUpdateEntry
            role={metadata.current.role}
            members={members}
            memberId={metadata.current.member_id}
            projectId={metadata.current.project_id}
            team={team}
          />
        );
      }

      if (
        metadata.previous &&
        metadata.previous.project_id &&
        metadata.previous.role &&
        metadata.previous.member_id
      ) {
        return (
          <ProjectRoleUpdateEntry
            role={metadata.previous.role}
            members={members}
            memberId={metadata.previous.member_id}
            projectId={metadata.previous.project_id}
            team={team}
            removed
          />
        );
      }

      captureMessage(`Found malformed metadata for ${action}`, "error");
      return <UnhandledAction action={action} />;

    case "team:join":
      return <span>joined the team</span>;
    case "team:create":
      return <span>created the team</span>;
    case "team:update":
      return <span>updated the team</span>;
    case "team:delete":
      return <span>deleted the team</span>;
    case "team:domain:create":
      return (
        <span>
          added {metadata.current?.domain ? "the" : "a"} domain{" "}
          {metadata.current?.domain && (
            <span className="font-semibold">{metadata.current.domain}</span>
          )}
        </span>
      );
    case "team:domain:delete":
      return (
        <span>
          removed {metadata.previous?.domain ? "the" : "a"} domain{" "}
          {metadata.previous?.domain && (
            <span className="font-semibold">{metadata.previous.domain}</span>
          )}
        </span>
      );
    case "deployment:create": {
      const deploymentType =
        metadata.current?.deploymentType ?? metadata.current?.type;
      if (!deploymentType || !metadata.current?.projectId) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }

      return (
        <span>
          created a <span className="font-semibold">{deploymentType}</span>{" "}
          deployment
          {metadata.current?.reference && (
            <>
              {" "}
              <span className="font-semibold">
                {metadata.current.reference}
              </span>
            </>
          )}{" "}
          for{" "}
          <ProjectLink
            projectId={metadata.current.projectId}
            metadata={metadata}
            team={team}
          />
        </span>
      );
    }
    case "deployment:delete": {
      const deploymentType =
        metadata.previous?.deploymentType ?? metadata.previous?.type;
      if (!deploymentType || !metadata.previous?.projectId) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          deleted a <span className="font-semibold">{deploymentType}</span>{" "}
          deployment
          {metadata.previous?.reference && (
            <>
              {" "}
              <span className="font-semibold">
                {metadata.previous.reference}
              </span>
            </>
          )}{" "}
          for{" "}
          <ProjectLink
            projectId={metadata.previous.projectId}
            metadata={metadata}
            team={team}
          />
        </span>
      );
    }
    case "deployment:update": {
      return (
        <span>
          updated deployment{" "}
          {metadata.current?.deploymentName && (
            <span className="font-semibold">
              {metadata.current.deploymentName}
            </span>
          )}
        </span>
      );
    }
    case "defaultEnvironmentVariable:create":
    case "defaultEnvironmentVariable:update":
    case "defaultEnvironmentVariable:delete":
      return (
        <EnvironmentVariableEntryAction
          action={action}
          metadata={metadata}
          team={team}
        />
      );
    case "billing:subscription:create":
      return (
        <span>subscribed to {metadata.current?.plan || "a Convex plan"}</span>
      );
    case "billing:subscription:cancel":
      return (
        <span>
          canceled the {metadata.previous?.plan || "Convex"} subscription
        </span>
      );
    case "billing:subscription:resume":
      return (
        <span>
          resumed the {metadata.current?.plan || "Convex"} subscription
        </span>
      );
    case "billing:subscription:changePlan":
      if (!metadata.previous?.plan || !metadata.current?.plan) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          changed the subscription plan from{" "}
          <span className="font-semibold">{metadata.previous?.plan}</span> to{" "}
          <span className="font-semibold">{metadata.current?.plan}</span>
        </span>
      );
    case "deployment:customDomain:create":
      return (
        <span>
          added {metadata.current?.domain ? "the" : "a"} custom domain{" "}
          {metadata.current?.domain && (
            <span className="font-semibold">{metadata.current.domain} </span>
          )}
          {metadata.current?.projectId && (
            <span>
              for{" "}
              <ProjectLink
                projectId={metadata.current?.projectId}
                metadata={metadata}
                team={team}
              />
            </span>
          )}
        </span>
      );
    case "deployment:customDomain:delete":
      return (
        <span>
          deleted {metadata.previous?.domain ? "the" : "a"} custom domain{" "}
          {metadata.previous?.domain && (
            <span className="font-semibold">{metadata.previous?.domain} </span>
          )}
          {metadata.previous?.projectId && (
            <span>
              for{" "}
              <ProjectLink
                projectId={metadata.previous?.projectId}
                metadata={metadata}
                team={team}
              />
            </span>
          )}
        </span>
      );
    case "team:token:create":
    case "project:token:create":
    case "deployment:token:create":
      return (
        <span>
          {metadata.current && (
            <AccessTokenSettingsLink
              team={team}
              metadataEntity={metadata.current}
              verb="created"
            />
          )}
        </span>
      );
    case "team:token:view":
    case "project:token:view":
    case "deployment:token:view":
      // we expect these to never be logged
      captureMessage("Found viewAccessToken audit log", "error");
      return (
        <span>
          {metadata.current && (
            <AccessTokenSettingsLink
              team={team}
              metadataEntity={metadata.current}
              verb="viewed"
            />
          )}
        </span>
      );
    case "member:token:createWithSsoAccess":
      return (
        <span>
          created a personal access token with SSO access to this team
        </span>
      );
    case "team:token:update":
    case "project:token:update":
    case "deployment:token:update":
      return (
        <span>
          {metadata.current && (
            <AccessTokenSettingsLink
              team={team}
              metadataEntity={metadata.current}
              verb="updated"
            />
          )}
        </span>
      );
    case "team:token:delete":
    case "project:token:delete":
    case "deployment:token:delete":
      return (
        <span>
          {metadata.previous && (
            <AccessTokenSettingsLink
              team={team}
              metadataEntity={metadata.previous}
              verb="deleted"
            />
          )}
        </span>
      );
    case "deployment:backups:create":
    case "deployment:backups:delete": {
      const verb =
        metadata.previous && metadata.current
          ? "updated"
          : metadata.previous
            ? "deleted"
            : "requested";
      const deploymentName =
        metadata.current?.sourceDeploymentName ||
        metadata.previous?.sourceDeploymentName;
      if (!deploymentName) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          {verb} a backup of{" "}
          <DeploymentSettingsLink
            team={team}
            deploymentName={deploymentName}
            urlSuffix="/backups"
          />
        </span>
      );
    }
    case "deployment:backups:import":
      if (
        !metadata.current?.targetDeploymentName ||
        !metadata.current?.backup ||
        !metadata.current?.backup?.sourceDeploymentName ||
        !metadata.current?.backup?.requestedTime
      ) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      return (
        <span>
          restored into{" "}
          <DeploymentSettingsLink
            team={team}
            deploymentName={metadata.current?.targetDeploymentName}
            urlSuffix="/backups"
          />{" "}
          from the backup <BackupIdentifier backup={metadata.current?.backup} />
        </span>
      );
    case "deployment:backups:configurePeriodic":
    case "deployment:backups:disablePeriodic": {
      if (
        !metadata.current?.sourceDeploymentName &&
        !metadata.previous?.sourceDeploymentName
      ) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      const verb =
        metadata.previous && metadata.current
          ? "updated"
          : metadata.previous
            ? "deleted"
            : "created";
      return (
        <span>
          {verb} a periodic backup schedule for{" "}
          <DeploymentSettingsLink
            team={team}
            deploymentName={
              metadata.current?.sourceDeploymentName ||
              metadata.previous?.sourceDeploymentName
            }
            urlSuffix="/backups"
          />{" "}
        </span>
      );
    }
    case "team:applyReferralCode": {
      return <span>applied a referral code</span>;
    }
    case "team:disableExceedingSpendingLimits": {
      return (
        <span>
          disabled your team's projects due to exceeding spending limits
        </span>
      );
    }
    case "billing:spendingLimit:update": {
      const { previous, current } = metadata;
      if (
        !isValidSpendingLimitDiff(previous) ||
        !isValidSpendingLimitDiff(current)
      ) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }

      return (
        <>
          <span>made a change to the spending limits:</span>
          <br />
          <div className="mt-2 inline-grid grid-cols-[auto_auto_auto_auto] items-center gap-x-2 gap-y-1.5">
            {previous.warningThresholdCents !==
              current.warningThresholdCents && (
              <SpendingLimitLine
                label="Warning threshold"
                tooltip="When your team exceeds this spending level, team admins will be notified."
                previousValue={previous.warningThresholdCents}
                currentValue={current.warningThresholdCents}
              />
            )}
            {previous.disableThresholdCents !==
              current.disableThresholdCents && (
              <SpendingLimitLine
                label="Disable threshold"
                tooltip="When your team exceeds this spending level, your projects will be disabled."
                previousValue={previous.disableThresholdCents}
                currentValue={current.disableThresholdCents}
              />
            )}
          </div>
        </>
      );
    }
    case "oauthApplication:verify": {
      return <span>verified an OAuth application</span>;
    }
    case "oauthApplication:delete": {
      return <span>deleted an OAuth application</span>;
    }
    case "oauthApplication:create": {
      return <span>created an OAuth application</span>;
    }
    case "oauthApplication:update": {
      return <span>updated an OAuth application</span>;
    }
    case "oauthApplication:generateClientSecret": {
      return <span>generated a client secret for an OAuth application</span>;
    }
    case "integration:workos:team:create": {
      return <span>created a WorkOS team</span>;
    }
    case "integration:workos:environment:create": {
      return <span>created a WorkOS environment</span>;
    }
    case "integration:workos:environment:retrieveCredentials": {
      return <span>retrieve WorkOS Environment credentials</span>;
    }
    case "integration:workos:team:disconnect": {
      return <span>disconnected a WorkOS team</span>;
    }
    case "integration:workos:environment:delete": {
      return <span>deleted a WorkOS environment</span>;
    }
    case "integration:workos:team:inviteMember": {
      return <span>invited a WorkOS team member</span>;
    }
    case "sso:enable": {
      return <span>enabled SSO</span>;
    }
    case "sso:disable": {
      return <span>disabled SSO</span>;
    }
    case "sso:update": {
      return <span>updated SSO settings</span>;
    }
    case "member:loginWithSso": {
      return <span>signed in with SSO</span>;
    }
    case "directorySync:enable": {
      return <span>enabled directory sync</span>;
    }
    case "directorySync:disable": {
      return <span>disabled directory sync</span>;
    }
    case "directorySync:updateGroupMapping": {
      const group = metadata.current?.group;
      return (
        <span>
          updated the role for directory group
          {group && (
            <>
              {" "}
              <span className="font-semibold">{group}</span>
            </>
          )}
        </span>
      );
    }
    case "directorySync:deleteGroupMapping": {
      const group = metadata.previous?.group;
      return (
        <span>
          removed the role mapping for directory group
          {group && (
            <>
              {" "}
              <span className="font-semibold">{group}</span>
            </>
          )}
        </span>
      );
    }
    case "integration:workos:projectEnvironment:create": {
      return <span>created a project WorkOS environment</span>;
    }
    case "integration:workos:projectEnvironment:delete": {
      return <span>deleted a project WorkOS environment</span>;
    }
    case "integration:workos:projectEnvironment:retrieveCredentials": {
      return <span>retrieved project WorkOS environment credentials</span>;
    }
    case "deployment:transfer": {
      return (
        <span>
          transferred deployment{" "}
          <span className="font-semibold">
            {metadata.previous?.reference ?? "unknown"}
          </span>{" "}
          from{" "}
          {metadata.previous?.projectId ? (
            <ProjectLink
              projectId={metadata.previous.projectId}
              metadata={metadata}
              team={team}
            />
          ) : (
            <span className="font-semibold">unknown project</span>
          )}{" "}
          to{" "}
          {metadata.current?.projectId ? (
            <ProjectLink
              projectId={metadata.current.projectId}
              metadata={metadata}
              team={team}
            />
          ) : (
            <span className="font-semibold">unknown project</span>
          )}
        </span>
      );
    }
    case "deployment:receive": {
      return <span>received a deployment from another project</span>;
    }
    case "customRole:create":
    case "customRole:update":
    case "customRole:delete": {
      const name = metadata.current?.name || metadata.previous?.name;
      if (!name) {
        captureMessage(`Found malformed metadata for ${action}`, "error");
        return <UnhandledAction action={action} />;
      }
      const verb =
        action === "customRole:create"
          ? "created"
          : action === "customRole:update"
            ? "updated"
            : "deleted";
      return (
        <span>
          {verb} the custom role <span className="font-semibold">{name}</span>
        </span>
      );
    }
    default:
      action satisfies never;
      captureMessage(`Unhandled audit log action: ${action}`, "error");
      return <UnhandledAction action={action} />;
  }
}

export function ProjectLink({
  metadata,
  team,
  projectId,
}: {
  projectId: number;
  metadata: AuditLogEntryMetadata;
  team: TeamResponse;
}) {
  const { project } = useProjectById(projectId);

  const projectName =
    project?.name ||
    (metadata.noun === "project"
      ? metadata.current?.name || metadata.previous?.name
      : "a deleted project");

  return project ? (
    <Link
      href={`/t/${team.slug}/${project.slug}/settings`}
      className="font-semibold"
      target="_blank"
    >
      {projectName}
    </Link>
  ) : (
    <span className="font-semibold text-content-secondary">{projectName}</span>
  );
}

function EnvironmentVariableEntryAction({
  team,
  action,
  metadata,
}: {
  team: TeamResponse;
  action: string;
  metadata: AuditLogEntryMetadata;
}) {
  if (!metadata.current?.projectId && !metadata.previous?.projectId) {
    captureMessage(`Found malformed metadata for ${action}`, "error");
    return <UnhandledAction action={action} />;
  }
  const verb =
    metadata.previous && metadata.current
      ? "updated"
      : metadata.previous
        ? "deleted"
        : "created";

  const variableName = metadata.current?.name || metadata.previous?.name;

  if (!variableName) {
    captureMessage(
      `Could not find variable name in metadata for ${action}`,
      "error",
    );
    return <UnhandledAction action={action} />;
  }

  return (
    <span>
      {verb} the environment variable{" "}
      <span className="font-semibold">{variableName}</span> in{" "}
      <ProjectLink
        projectId={metadata.current?.projectId || metadata.previous?.projectId}
        metadata={metadata}
        team={team}
      />
    </span>
  );
}

function ProjectEntryAction({
  team,
  action,
  metadata,
}: {
  team: TeamResponse;
  action: string;
  metadata: AuditLogEntryMetadata;
}) {
  if (!metadata.current?.id && !metadata.previous?.id) {
    captureMessage(`Found malformed metadata for ${action}`, "error");
    return <UnhandledAction action={action} />;
  }

  const verb =
    metadata.previous && metadata.current
      ? "updated"
      : metadata.previous
        ? "deleted"
        : "created";

  return (
    <span>
      {verb} the project{" "}
      <ProjectLink
        projectId={metadata.current?.id || metadata.previous?.id}
        metadata={metadata}
        team={team}
      />
    </span>
  );
}

function UnhandledAction({ action }: { action: string }) {
  return (
    <span>
      performed the action{" "}
      <span className="font-semibold">{startCase(action)}</span>
    </span>
  );
}

function ProjectRoleUpdateEntry({
  role,
  members,
  memberId,
  projectId,
  team,
  removed = false,
}: {
  members: MemberResponse[];
  role?: "admin";
  memberId: number;
  projectId: number;
  team: TeamResponse;
  removed?: boolean;
}) {
  return (
    <span>
      {removed ? "removed" : "gave"} the{" "}
      <span className="font-semibold">Project {startCase(role)}</span> role{" "}
      {removed ? "from" : "to"}{" "}
      <TeamMemberLink
        memberId={memberId}
        name={
          members.find((m) => m.id === memberId)?.name || memberId.toString()
        }
      />{" "}
      for{" "}
      <ProjectLink
        projectId={projectId}
        // Don't need metadata for this project link
        metadata={{}}
        team={team}
      />
    </span>
  );
}

function AuditLogItemActor({
  entry,
  memberId,
  members,
}: {
  entry: AuditLogEventResponse;
  memberId: number | null;
  members: MemberResponse[];
}) {
  if (entry.actor.kind === "system") {
    return <span className="font-semibold">Convex</span>;
  }
  if (entry.actor.kind === "integration") {
    return <span className="font-semibold">An OAuth application</span>;
  }
  // A token actor with no associated member is a team-level deploy key.
  if (
    entry.actor.kind === "token" &&
    (entry.actor.member_id === null || entry.actor.member_id === undefined)
  ) {
    return <span className="font-semibold">A Deploy Key</span>;
  }
  const member = members?.find((m) => m.id === memberId);
  return member ? (
    <TeamMemberLink memberId={member.id} name={member.name || member.email} />
  ) : (entry.metadata as AuditLogEntryMetadata).noun === "member" ? (
    <span className="font-semibold">
      {(entry.metadata as AuditLogEntryMetadata)?.current?.email ||
        (entry.metadata as AuditLogEntryMetadata)?.previous?.email}
    </span>
  ) : (
    <span className="font-semibold">
      A team member{" "}
      <span className="font-normal text-content-secondary">
        (Member ID: {memberId})
      </span>
    </span>
  );
}

function deploymentDisplayName(deployment: PlatformDeploymentResponse) {
  if (deployment.kind === "cloud") {
    return `${deployment.reference}`;
  }
  switch (deployment.deploymentType) {
    case "prod":
      return "a production deployment";
    case "dev":
      return "a development deployment";
    case "preview":
      return "a preview deployment";
    case "custom":
      return "a custom deployment";
    default:
      deployment.deploymentType satisfies never;
      return "a deployment";
  }
}
function DeploymentSettingsLink({
  team,
  deploymentName,
  urlSuffix = "",
}: {
  team: TeamResponse;
  deploymentName: string;
  urlSuffix?: string;
}) {
  const deployment = useDeploymentByName(deploymentName);
  const { project, isLoading: isLoadingProject } = useProjectById(
    deployment?.projectId,
  );

  if (!deployment) {
    return <span>a deployment</span>;
  }

  if (isLoadingProject) {
    return <span>a deployment</span>;
  }

  if (!project) {
    captureMessage(
      `Malformed deploy key audit log entry:
      deployment ${deploymentName} has project id ${deployment.projectId}
      which is not found within the projects of team ${team.id}`,
      "error",
    );
    return <span>a deployment</span>;
  }

  return (
    <>
      <Link
        href={`/t/${team.slug}/${project.slug}/${deployment.name}/settings${urlSuffix}`}
        className="font-semibold"
        target="_blank"
      >
        {deploymentDisplayName(deployment)}
      </Link>
      <span> in {project.name}</span>
    </>
  );
}

function ProjectSettingsLink({
  team,
  projectId,
}: {
  team: TeamResponse;
  projectId: number;
}) {
  const { project, isLoading } = useProjectById(projectId);
  if (isLoading || !project) {
    return <span>Project {projectId}</span>;
  }

  return (
    <Link
      href={`/t/${team.slug}/${project.slug}/settings`}
      className="font-semibold"
      target="_blank"
    >
      {project.name}
    </Link>
  );
}

function AccessTokenSettingsLink({
  team,
  metadataEntity,
  verb,
}: {
  team: TeamResponse;
  metadataEntity: Record<string, any>;
  verb: string;
}) {
  const keyType = metadataEntity.deploymentId
    ? "deploy key"
    : metadataEntity.projectId
      ? "preview deploy key"
      : "access token";

  return (
    <>
      {verb} the {keyType}{" "}
      <span className="font-semibold">{metadataEntity.name}</span>
      {metadataEntity.projectId && (
        <>
          {" "}
          in{" "}
          <ProjectSettingsLink
            team={team}
            projectId={metadataEntity?.projectId}
          />
        </>
      )}
    </>
  );
}

type SpendingLimitDiff = {
  warningThresholdCents: number | null;
  disableThresholdCents: number | null;
};

function isValidSpendingLimitDiff(value: unknown): value is SpendingLimitDiff {
  if (typeof value !== "object" || value === null) {
    return false;
  }

  return (
    "warningThresholdCents" in value &&
    "disableThresholdCents" in value &&
    (typeof value.warningThresholdCents === "number" ||
      value.warningThresholdCents === null) &&
    (typeof value.disableThresholdCents === "number" ||
      value.disableThresholdCents === null)
  );
}

function SpendingLimitLine({
  label,
  tooltip,
  previousValue,
  currentValue,
}: {
  label: string;
  tooltip: string;
  previousValue: number | null;
  currentValue: number | null;
}) {
  return (
    <div className="contents">
      <div className="mr-2 flex items-center gap-1">
        <div className="text-content-secondary">{label}</div>
        <HelpTooltip tipSide="top">{tooltip}</HelpTooltip>
      </div>
      <SpendingValue valueCents={previousValue} />
      <ArrowRightIcon className="text-content-tertiary" />
      <SpendingValue valueCents={currentValue} />
    </div>
  );
}

function SpendingValue({ valueCents }: { valueCents: number | null }) {
  return (
    <div className="text-right font-medium text-content-primary tabular-nums">
      {valueCents === null ? "None" : formatUsd(valueCents / 100)}
    </div>
  );
}
