import { DeleteProjectModal } from "components/projects/modals/DeleteProjectModal";
import { PageContent } from "@common/elements/PageContent";
import { NoPermissionMessage } from "elements/NoPermissionMessage";
import { Loading } from "@ui/Loading";
import { Button } from "@ui/Button";
import { Sheet } from "@ui/Sheet";
import { useDeployments } from "api/deployments";
import { useCurrentTeam, useTeamEntitlements } from "api/teams";
import { useCurrentProject } from "api/projects";
import {
  useCreatePreviewDeployKey,
  useDeletePreviewDeployKey,
  usePreviewDeployKeys,
  useProjectAppAccessTokens,
  useDeleteAppAccessTokenByName,
} from "api/accessTokens";
import {
  useHasCustomRolePermission,
  useHasProjectAdminPermissions,
} from "api/roles";
import { useProfile } from "api/profile";
import { projectResource, projectTokenResource } from "lib/permissions";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { useRouter } from "next/router";
import { useState } from "react";
import { ProjectForm } from "components/projects/ProjectForm";
import {
  GearIcon,
  GlobeIcon,
  Link2Icon,
  PersonIcon,
  PieChartIcon,
  TrashIcon,
} from "@radix-ui/react-icons";
import {
  ArrowsRightLeftIcon,
  KeyIcon,
  VariableIcon,
} from "@heroicons/react/24/outline";
import { withAuthenticatedPage } from "lib/withAuthenticatedPage";
import { DefaultEnvironmentVariables } from "components/projectSettings/DefaultEnvironmentVariables";
import { ProjectDetails } from "generatedApi";
import { Link } from "@ui/Link";
import Head from "next/head";
import { MemberProjectRoles } from "components/projects/MemberProjectRoles";
import { DeploymentAccessTokenList } from "components/deploymentSettings/DeploymentAccessTokenList";
import { CustomDomains } from "components/projectSettings/CustomDomains";
import { TransferProject } from "components/projects/TransferProject";
import { AuthorizedApplications } from "components/AuthorizedApplications";
import { HelpTooltip } from "@ui/HelpTooltip";
import { PROJECT_SETTINGS_SECTIONS } from "lib/sectionAnchors";
import { SettingsLayout, type SettingsSection } from "elements/SettingsLayout";

export { getServerSideProps } from "lib/ssr";

export function ProjectSettingsPage() {
  return (
    <PageContent>
      <ProjectSettings />
    </PageContent>
  );
}

export default withAuthenticatedPage(ProjectSettingsPage);

// Anchor ids are shared with the command palette (which deep-links to them)
// via `lib/sectionAnchors`, so the two can't drift.
const SECTION_IDS = {
  projectForm: PROJECT_SETTINGS_SECTIONS.editProject.id,
  projectRoles: PROJECT_SETTINGS_SECTIONS.projectAdmins.id,
  projectUsage: PROJECT_SETTINGS_SECTIONS.projectUsage.id,
  customDomains: PROJECT_SETTINGS_SECTIONS.customDomains.id,
  previewDeployKeys: PROJECT_SETTINGS_SECTIONS.previewDeployKeys.id,
  authorizedApps: PROJECT_SETTINGS_SECTIONS.authorizedApplications.id,
  envVars: PROJECT_SETTINGS_SECTIONS.environmentVariables.id,
  transferProject: PROJECT_SETTINGS_SECTIONS.transferProject.id,
  deleteProject: PROJECT_SETTINGS_SECTIONS.deleteProject.id,
} as const;

const sections: SettingsSection[] = [
  { id: SECTION_IDS.projectForm, label: "Edit Project", Icon: GearIcon },
  { id: SECTION_IDS.projectRoles, label: "Project Admins", Icon: PersonIcon },
  { id: SECTION_IDS.projectUsage, label: "Project Usage", Icon: PieChartIcon },
  { id: SECTION_IDS.customDomains, label: "Custom Domains", Icon: GlobeIcon },
  {
    id: SECTION_IDS.previewDeployKeys,
    label: "Preview Deploy Keys",
    Icon: KeyIcon,
  },
  {
    id: SECTION_IDS.authorizedApps,
    label: "Authorized Applications",
    Icon: Link2Icon,
  },
  {
    id: SECTION_IDS.envVars,
    label: "Default Environment Variables",
    Icon: VariableIcon,
  },
  {
    id: SECTION_IDS.transferProject,
    label: "Transfer Project",
    Icon: ArrowsRightLeftIcon,
  },
  { id: SECTION_IDS.deleteProject, label: "Delete Project", Icon: TrashIcon },
];

function ProjectSettings() {
  const team = useCurrentTeam();
  const project = useCurrentProject();
  const entitlements = useTeamEntitlements(team?.id);
  const hasAdminPermissions = useHasProjectAdminPermissions(project?.id);

  // Custom-role gates: project admins (and team admins via
  // `hasAdminPermissions`) keep full access, custom-role members opt in via
  // explicit grants. The non-custom-role result is `false` because the
  // built-in role check happens via `hasAdminPermissions` above.
  const projResource = project ? projectResource(project) : undefined;
  const canUpdateProjectCustom = useHasCustomRolePermission(
    team?.id,
    "project:update",
    projResource,
    false,
  );
  const canEditProject = hasAdminPermissions || canUpdateProjectCustom === true;
  // `/projects/{id}/app_access_tokens` is gated server-side on
  // `project:token:view`; skip the fetch when the member can't view (whole
  // list, so token-creator selector is null) and surface a clear message.
  const canViewAppTokenCustom = useHasCustomRolePermission(
    team?.id,
    "project:token:view",
    project ? projectTokenResource(project, null) : undefined,
    true,
  );
  const canViewAuthorizedApps =
    hasAdminPermissions || canViewAppTokenCustom === true;
  // Gate the NoPermissionMessage on explicit denial so the section
  // doesn't flicker into "no permission" while role data resolves.
  const isAuthorizedAppsDenied =
    !hasAdminPermissions && canViewAppTokenCustom === false;
  const canDeleteAppTokenCustom = useHasCustomRolePermission(
    team?.id,
    "project:token:delete",
    project ? projectTokenResource(project, null) : undefined,
    false,
  );
  const canRevokeAuthorizedApp =
    hasAdminPermissions || canDeleteAppTokenCustom === true;

  const projectAppAccessTokens = useProjectAppAccessTokens(
    canViewAuthorizedApps ? project?.id : undefined,
  );
  const deleteAppAccessTokenByName = useDeleteAppAccessTokenByName({
    projectId: project?.id,
  });

  const authorizedAppsExplainer = (
    <>
      <h3 className="mb-2">Authorized Applications</h3>
      <p className="text-sm text-content-primary">
        These 3rd-party applications have been authorized to access this project
        on your behalf.
      </p>
      <div className="my-2 text-sm text-content-primary">
        <span className="font-semibold">
          What can authorized applications do?
        </span>
        <ul className="mt-1 list-disc pl-4">
          <li>Create new deployments in this project</li>
          <li>
            <span className="flex items-center gap-1">
              Manage this project
              <HelpTooltip>
                This includes actions like managing custom domains, managing
                environment variable defaults, and managing cloud backups and
                restores.
              </HelpTooltip>
            </span>
          </li>
          <li>
            <span className="flex items-center gap-1">
              Read and write data in any deployment in this project
              <HelpTooltip>
                Write access to Production deployments will depend on your
                team-level and project-level roles.
              </HelpTooltip>
            </span>
          </li>
        </ul>
      </div>
      <p className="mt-1 mb-2 text-sm text-content-primary">
        You cannot see applications that other members of your team have
        authorized.
      </p>
      {team && (
        <p className="mt-1 mb-2 text-xs text-content-secondary">
          There may also be <b>team-wide authorized applications</b> that can
          access all projects in this team. You can view them in{" "}
          <Link href={`/t/${team.slug}/settings/applications`}>
            Team Settings
          </Link>
          .
        </p>
      )}
    </>
  );

  return (
    <>
      <Head>
        {project && (
          <title>Project Settings | {project.name} | Convex Dashboard</title>
        )}
      </Head>
      <SettingsLayout
        title="Project Settings"
        sections={sections}
        contentReady={!!(team && project)}
      >
        {team && project ? (
          <div id={SECTION_IDS.projectForm}>
            <ProjectForm
              team={team}
              project={project}
              hasAdminPermissions={canEditProject}
              permissionDeniedTip={
                canEditProject
                  ? undefined
                  : permissionDeniedTip(
                      "You do not have permission to update this project.",
                      "project:update",
                    )
              }
            />
          </div>
        ) : (
          <Loading className="h-200" fullHeight={false} />
        )}
        <div id={SECTION_IDS.projectRoles}>
          <MemberProjectRoles />
        </div>
        {team && project && (
          <Sheet id={SECTION_IDS.projectUsage}>
            <h3 className="mb-4">Project Usage</h3>
            <p className="text-sm">
              View this project's usage and limits on{" "}
              <Link
                href={`/t/${team.slug}/settings/usage?projectSlug=${project.slug}`}
              >
                this team's usage page
              </Link>
              .
            </p>
          </Sheet>
        )}
        {team && entitlements && (
          <div id={SECTION_IDS.customDomains}>
            <CustomDomains
              team={team}
              hasEntitlement={entitlements.customDomainsEnabled ?? false}
            />
          </div>
        )}
        {project && (
          <div id={SECTION_IDS.previewDeployKeys}>
            <PreviewDeployKeys project={project} />
          </div>
        )}
        {project && (
          <div id={SECTION_IDS.authorizedApps}>
            {isAuthorizedAppsDenied ? (
              <Sheet>
                <h3 className="mb-2">Authorized Applications</h3>
                <NoPermissionMessage
                  message="You do not have permission to view authorized applications for this project."
                  missingPermission="project:token:view"
                />
              </Sheet>
            ) : (
              <AuthorizedApplications
                accessTokens={projectAppAccessTokens}
                explainer={authorizedAppsExplainer}
                onRevoke={async (token) => {
                  await deleteAppAccessTokenByName({
                    name: token.name,
                  });
                }}
                revokeDisabledReason={
                  canRevokeAuthorizedApp
                    ? undefined
                    : permissionDeniedTip(
                        "You do not have permission to revoke authorized applications.",
                        "project:token:delete",
                      )
                }
              />
            )}
          </div>
        )}
        <div id={SECTION_IDS.envVars}>
          <DefaultEnvironmentVariables />
        </div>
        <div id={SECTION_IDS.transferProject}>
          <TransferProject />
        </div>
        <div id={SECTION_IDS.deleteProject}>
          <DeleteProject />
        </div>
      </SettingsLayout>
    </>
  );
}

function DeleteProject() {
  const router = useRouter();

  const team = useCurrentTeam();
  const project = useCurrentProject();

  const hasAdminPermissions = useHasProjectAdminPermissions(project?.id);
  const canDeleteCustom = useHasCustomRolePermission(
    team?.id,
    "project:delete",
    project ? projectResource(project) : undefined,
    false,
  );
  const canDelete = hasAdminPermissions || canDeleteCustom === true;

  const [showDeleteModal, setShowDeleteModal] = useState(false);
  return (
    <>
      <Sheet>
        <h3 className="mb-4">Delete Project</h3>
        <p className="mb-5 text-sm text-content-primary">
          Permanently delete this project for you and all team members. This
          action cannot be undone.
        </p>
        <Button
          variant="danger"
          onClick={() => setShowDeleteModal(!showDeleteModal)}
          icon={<TrashIcon />}
          disabled={!canDelete}
          tip={
            !canDelete
              ? permissionDeniedTip(
                  "You do not have permission to delete this project.",
                  "project:delete",
                )
              : undefined
          }
        >
          Delete
        </Button>
      </Sheet>
      {team && project && showDeleteModal && (
        <DeleteProjectModal
          team={team}
          project={project}
          onClose={() => setShowDeleteModal(false)}
          onDelete={async () => {
            await router.push("/");
          }}
        />
      )}
    </>
  );
}

function PreviewDeployKeys({ project }: { project: ProjectDetails }) {
  const createPreviewDeployKey = useCreatePreviewDeployKey(project.id);
  const deletePreviewDeployKey = useDeletePreviewDeployKey(project.id);
  const team = useCurrentTeam();
  const profile = useProfile();
  const hasAdminPermissions = useHasProjectAdminPermissions(project.id);

  const { deployments } = useDeployments(project.id);
  const defaultProdDeployment = deployments?.find(
    (d) => d.kind === "cloud" && d.deploymentType === "prod" && d.isDefault,
  );

  // Listing preview deploy keys requires `project:token:view`; whole-list
  // checks scope the token resource to `creator=null` (no `creator=self`
  // role would let you see other members' tokens).
  const canViewCustom = useHasCustomRolePermission(
    team?.id,
    "project:token:view",
    projectTokenResource(project, null),
    true,
  );
  const canView = hasAdminPermissions || canViewCustom === true;
  // Only render the NoPermissionMessage on explicit denial so the section
  // doesn't flash for everyone while role data is still resolving.
  const isViewDenied = !hasAdminPermissions && canViewCustom === false;

  // Project-scoped token creation: a role like `token:creator=me` should
  // still let the member generate their own preview deploy keys, so scope
  // the create-resource to the current member id.
  const canCreateCustom = useHasCustomRolePermission(
    team?.id,
    "project:token:create",
    projectTokenResource(project, profile?.id ?? null),
    false,
  );
  const canCreate = hasAdminPermissions || canCreateCustom === true;
  const disabledReason: "NoPermissionForPreview" | null = !canCreate
    ? "NoPermissionForPreview"
    : null;

  const previewDeployKeys = usePreviewDeployKeys(
    canView ? project.id : undefined,
  );

  const deployKeyDescription = (
    <p className="mb-2 max-w-prose text-sm text-content-primary">
      These keys are for creating{" "}
      <Link
        passHref
        href="https://docs.convex.dev/production/multiple-deployments#preview"
        target="_blank"
      >
        preview deployments
      </Link>
      . Generate and copy a preview key to integrate Convex with a{" "}
      <Link
        passHref
        href="https://docs.convex.dev/production/hosting"
        target="_blank"
      >
        hosting provider
      </Link>{" "}
      (like Netlify or Vercel) in order to view both frontend and backend
      changes before they're deployed to production.
    </p>
  );

  if (isViewDenied) {
    return (
      <Sheet className="flex flex-col gap-4">
        <h3>Preview Deploy Keys</h3>
        <NoPermissionMessage
          message="You do not have permission to view preview deploy keys for this project."
          missingPermission="project:token:view"
        />
      </Sheet>
    );
  }

  return (
    <Sheet className="flex flex-col gap-4">
      <div className="flex flex-col gap-2">
        {team && (
          <DeploymentAccessTokenList
            deploymentType="preview"
            onDelete={deletePreviewDeployKey}
            deployKeys={previewDeployKeys}
            disabledReason={disabledReason}
            buttonProps={{
              deploymentType: "preview",
              disabledReason,
              showCustomPermissions: false,
              getAdminKey: async (
                name: string,
                _allowedActions: string[] | undefined,
                expiresAt: number | undefined,
              ) => {
                try {
                  const result = await createPreviewDeployKey({
                    name,
                    ...(expiresAt !== undefined && { expiresAt }),
                  });
                  if (!result)
                    return {
                      ok: false as const,
                      error: "Failed to create preview deploy key.",
                    };
                  return {
                    ok: true as const,
                    adminKey: result.previewDeployKey,
                  };
                } catch (e) {
                  return {
                    ok: false as const,
                    error:
                      (e as { message?: string })?.message ??
                      "Failed to create preview deploy key.",
                  };
                }
              },
            }}
            header="Preview Deploy Keys"
            headingLevel="h3"
            description={deployKeyDescription}
          />
        )}
      </div>
      <p className="max-w-prose text-xs text-content-secondary">
        Looking for Production Deploy Keys? You can manage your Production
        deploy keys in your{" "}
        {team && defaultProdDeployment ? (
          <Link
            href={`/t/${team.slug}/${project.slug}/${defaultProdDeployment.name}/settings`}
          >
            Production Deployment Settings
          </Link>
        ) : (
          <span className="font-semibold">Production Deployment Settings</span>
        )}
        .
      </p>
    </Sheet>
  );
}
