import { TextInput } from "@ui/TextInput";
import {
  integrationCategory,
  integrationName,
} from "@common/lib/integrationHelpers";
import {
  useHasCustomRolePermission,
  useIsCurrentMemberTeamAdmin,
} from "api/roles";
import { TeamResponse } from "generatedApi";
import { useLaunchDarkly } from "hooks/useLaunchDarkly";
import { INTEGRATION_RESOURCE } from "lib/permissions";
import { ReactNode, useState } from "react";
import { IntegrationType } from "system-udfs/convex/_system/frontend/common";
import { DeploymentIntegrationCard } from "./CatalogCard";
import { DEPLOYMENT_INTEGRATIONS } from "./deploymentIntegrations";
import {
  WORKOS_CATALOG_ENTRY,
  WorkOSTeamIntegrationCard,
} from "./WorkOSTeamIntegrationCard";

export function TeamIntegrations({ team }: { team: TeamResponse }) {
  const [search, setSearch] = useState("");
  const { s3ExportIntegration, teamIntegrationsPage } = useLaunchDarkly();
  const isTeamAdmin = useIsCurrentMemberTeamAdmin();
  const canConnectCustom = useHasCustomRolePermission(
    team.id,
    "integration:create",
    INTEGRATION_RESOURCE,
    false,
  );
  const canDisconnectCustom = useHasCustomRolePermission(
    team.id,
    "integration:delete",
    INTEGRATION_RESOURCE,
    false,
  );

  if (!teamIntegrationsPage) {
    return null;
  }

  const query = search.trim().toLowerCase();
  const matches = (...fields: string[]) =>
    fields.some((field) => field.toLowerCase().includes(query));

  const showWorkOS = matches(
    WORKOS_CATALOG_ENTRY.name,
    WORKOS_CATALOG_ENTRY.category,
    WORKOS_CATALOG_ENTRY.description,
  );
  const deploymentIntegrations = (
    Object.entries(DEPLOYMENT_INTEGRATIONS) as [
      Exclude<IntegrationType, "workos">,
      (typeof DEPLOYMENT_INTEGRATIONS)[keyof typeof DEPLOYMENT_INTEGRATIONS],
    ][]
  )
    .filter(([kind]) => kind !== "s3Export" || s3ExportIntegration)
    .filter(([kind, { description }]) =>
      matches(integrationName(kind), integrationCategory(kind), description),
    );

  return (
    <div className="flex flex-col gap-6">
      <div className="flex flex-wrap items-end justify-between gap-4">
        <div>
          <h2>Integrations</h2>
          <p className="mt-1 max-w-prose text-sm text-content-secondary">
            Connect Convex to the services your team uses.
          </p>
        </div>
        <div className="w-full sm:w-64">
          <TextInput
            type="search"
            id="integrationSearch"
            labelHidden
            label="Search integrations"
            placeholder="Search integrations"
            value={search}
            onChange={(e) => setSearch(e.target.value)}
          />
        </div>
      </div>
      {showWorkOS && (
        <CatalogSection>
          <WorkOSTeamIntegrationCard
            teamId={team.id}
            canConnect={isTeamAdmin || canConnectCustom === true}
            canDisconnect={isTeamAdmin || canDisconnectCustom === true}
          />
        </CatalogSection>
      )}
      {deploymentIntegrations.length > 0 && (
        <CatalogSection
          title="Other Integrations"
          description="Set these up separately in each deployment's Settings › Integrations."
        >
          {deploymentIntegrations.map(([kind, { description, docsUrl }]) => (
            <DeploymentIntegrationCard
              key={kind}
              kind={kind}
              description={description}
              docsUrl={docsUrl}
            />
          ))}
        </CatalogSection>
      )}
      {!showWorkOS && deploymentIntegrations.length === 0 && (
        <p className="text-content-secondary">
          No integrations match &ldquo;{search.trim()}&rdquo;.
        </p>
      )}
    </div>
  );
}

function CatalogSection({
  title,
  description,
  children,
}: {
  title?: string;
  description?: string;
  children: ReactNode;
}) {
  return (
    <section className="flex flex-col gap-3">
      {title && (
        <div>
          <h3>{title}</h3>
          <p className="mt-1 text-sm text-content-secondary">{description}</p>
        </div>
      )}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2 xl:grid-cols-3">
        {children}
      </div>
    </section>
  );
}
