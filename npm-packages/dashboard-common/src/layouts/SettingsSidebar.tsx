import classNames from "classnames";
import React, { useContext } from "react";
import {
  PieChartIcon,
  ExternalLinkIcon,
  GearIcon,
} from "@radix-ui/react-icons";
import { DeploymentPageTitle } from "@common/elements/DeploymentPageTitle";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import { SidebarLink } from "@common/elements/Sidebar";
import { useNents } from "@common/lib/useNents";
import { useIsCloudDeploymentInSelfHostedDashboard } from "@common/lib/useIsCloudDeploymentInSelfHostedDashboard";
import {
  DEPLOYMENT_SETTINGS_PAGES_AND_NAMES,
  DEPLOYMENT_SETTINGS_PAGE_ICONS,
  SettingsPageKind,
} from "@common/layouts/deploymentSettingsPages";

export {
  DEPLOYMENT_SETTINGS_PAGES_AND_NAMES,
  type SettingsPageKind,
} from "@common/layouts/deploymentSettingsPages";

const DEPLOYMENT_SETTINGS_PAGES = Object.keys(
  DEPLOYMENT_SETTINGS_PAGES_AND_NAMES,
) as SettingsPageKind[];

export function SettingsSidebar({
  selectedPage,
}: {
  selectedPage: SettingsPageKind;
}) {
  const allowedPages = useAllowedPages();

  const {
    isSelfHosted,
    useCurrentProject,
    useCurrentDeployment,
    teamsURI,
    projectsURI,
    deploymentsURI,
  } = useContext(DeploymentInfoContext);

  const project = useCurrentProject();
  const deployment = useCurrentDeployment();

  const { isCloudDeploymentInSelfHostedDashboard, deploymentName } =
    useIsCloudDeploymentInSelfHostedDashboard();
  const isSelfHostedDeployment =
    isSelfHosted && !isCloudDeploymentInSelfHostedDashboard;

  return (
    <>
      <DeploymentPageTitle
        title={DEPLOYMENT_SETTINGS_PAGES_AND_NAMES[selectedPage]}
      />
      <div
        className={classNames(
          "flex gap-2 h-full min-w-60 flex-col bg-background-secondary md:border-r md:p-4 overflow-y-auto scrollbar",
        )}
      >
        {/* On larger screens, this is a sidebar and not a popover menu. */}
        {allowedPages.map((page) => {
          const showInCloudDashboard =
            page === "backups" && isCloudDeploymentInSelfHostedDashboard;
          const isUnavailableForSelfHosted =
            (page === "backups" || page === "custom-domains") &&
            isSelfHostedDeployment;
          const isUnavailableForLocal =
            (page === "backups" || page === "custom-domains") &&
            deployment?.kind === "local";

          return (
            <SidebarLink
              href={
                showInCloudDashboard
                  ? `https://dashboard.convex.dev/d/${deploymentName}/settings/${page}`
                  : `${deploymentsURI}/settings/${page === "general" ? "" : page}`
              }
              Icon={DEPLOYMENT_SETTINGS_PAGE_ICONS[page]}
              isActive={page === selectedPage}
              key={page}
              disabled={isUnavailableForSelfHosted || isUnavailableForLocal}
              tip={
                isUnavailableForLocal
                  ? `The ${DEPLOYMENT_SETTINGS_PAGES_AND_NAMES[page]} feature is not available in local deployments.`
                  : isUnavailableForSelfHosted
                    ? `The ${DEPLOYMENT_SETTINGS_PAGES_AND_NAMES[page]} feature is not currently available in self-hosted deployments.`
                    : undefined
              }
              target={showInCloudDashboard ? "_blank" : undefined}
            >
              <div className="flex items-center justify-between gap-2">
                {DEPLOYMENT_SETTINGS_PAGES_AND_NAMES[page]}
                {showInCloudDashboard && <ExternalLinkIcon />}
              </div>
            </SidebarLink>
          );
        })}
        <div className="flex flex-col gap-2 border-t py-2">
          <SidebarLink
            href={
              isCloudDeploymentInSelfHostedDashboard
                ? `https://dashboard.convex.dev/dp/${deploymentName}/settings`
                : `${projectsURI}/settings`
            }
            Icon={GearIcon}
            isActive={false}
            disabled={isSelfHostedDeployment}
            tip={
              isSelfHostedDeployment
                ? "Project settings are not available in self-hosted deployments."
                : undefined
            }
            target={
              isCloudDeploymentInSelfHostedDashboard ? "_blank" : undefined
            }
          >
            <div className="flex items-center justify-between gap-2">
              Project Settings
              <ExternalLinkIcon />
            </div>
          </SidebarLink>
          <SidebarLink
            href={
              isCloudDeploymentInSelfHostedDashboard
                ? `https://dashboard.convex.dev/dp/${deploymentName}/usage`
                : `${teamsURI}/settings/usage`
            }
            Icon={PieChartIcon}
            query={
              isCloudDeploymentInSelfHostedDashboard
                ? {}
                : {
                    projectSlug: project?.slug ?? "",
                  }
            }
            isActive={false}
            disabled={isSelfHostedDeployment}
            tip={
              isSelfHostedDeployment
                ? "Project usage is not available in self-hosted deployments."
                : undefined
            }
            target={
              isCloudDeploymentInSelfHostedDashboard ? "_blank" : undefined
            }
          >
            <div className="flex items-center justify-between gap-2">
              Project Usage
              <ExternalLinkIcon />
            </div>
          </SidebarLink>
        </div>
      </div>
    </>
  );
}

function useAllowedPages() {
  const { nents } = useNents();

  let pages = DEPLOYMENT_SETTINGS_PAGES;

  if (!nents || nents.length === 0) {
    pages = pages.filter((d) => d !== "components");
  }

  pages = pages.filter((d) => d !== "snapshots");

  return pages;
}
