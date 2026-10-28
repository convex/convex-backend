import { LocalDevCallout } from "@common/elements/LocalDevCallout";
import { Callout } from "@ui/Callout";
import { Sheet } from "@ui/Sheet";
import { useDeployments } from "api/deployments";
import { useCurrentProject } from "api/projects";
import { Link } from "@ui/Link";
import { ReactNode } from "react";
import { TeamResponse } from "generatedApi";
import { DeploymentInfoProvider } from "providers/DeploymentInfoProvider";
import { MaybeDeploymentApiProvider } from "providers/MaybeDeploymentApiProvider";
import { WaitForDeploymentApi } from "@common/lib/deploymentContext";

export { CanonicalUrlCombobox } from "../deploymentSettings/CustomDomains";

export function CustomDomains({
  team,
  hasEntitlement,
}: {
  team: TeamResponse;
  hasEntitlement: boolean;
}) {
  const project = useCurrentProject();
  const defaultProdDeployment = useDeployments(project?.id).deployments?.find(
    (d) => d.kind === "cloud" && d.deploymentType === "prod" && d.isDefault,
  );

  return (
    <Sheet>
      <div className="flex flex-col gap-4">
        <div>
          <h3 className="mb-2">Custom Domains</h3>
          <p className="max-w-prose">
            Configuration for Custom domains has moved. You may configure Custom
            Domains in{" "}
            {defaultProdDeployment && project ? (
              <Link
                href={`/t/${team.slug}/${project.slug}/${defaultProdDeployment.name}/settings/custom-domains`}
              >
                Deployment Settings
              </Link>
            ) : (
              <span className="font-semibold">Deployment Settings</span>
            )}
            .
          </p>
        </div>

        <div>
          {!hasEntitlement && (
            <>
              <Callout>
                <div>
                  Custom domains are{" "}
                  <span className="font-semibold">
                    only available on the Pro plan
                  </span>
                  .{" "}
                  <Link href={`/t/${team.slug}/settings/billing`}>
                    Upgrade to get access.
                  </Link>
                </div>
              </Callout>
              <LocalDevCallout
                className="flex-col"
                tipText="Tip: Run this to enable custom domains locally:"
                command={`just big-brain-tool-dev entitlement grant add --team-entitlement custom_domains_enabled --team-id ${team.id} --reason "local" true --for-real && just big-brain-tool-dev entitlement grant add --team-entitlement max_custom_domains --team-id ${team.id} --reason "local" 100 --for-real`}
              />
            </>
          )}
        </div>
      </div>
    </Sheet>
  );
}

export function DeploymentProvider({
  children,
  deploymentName,
  deploymentUrl,
}: {
  children: ReactNode;
  deploymentName: string;
  deploymentUrl?: string;
}) {
  return (
    <DeploymentInfoProvider
      deploymentOverride={deploymentName}
      deploymentUrlOverride={deploymentUrl}
    >
      <MaybeDeploymentApiProvider deploymentOverride={deploymentName}>
        <WaitForDeploymentApi sizeClass="hidden">
          {children}
        </WaitForDeploymentApi>
      </MaybeDeploymentApiProvider>
    </DeploymentInfoProvider>
  );
}
