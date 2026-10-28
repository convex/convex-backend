import { useFlags } from "launchdarkly-react-client-sdk";
import kebabCase from "lodash/kebabCase";

export const flagDefaults: {
  commandPaletteDeleteProjects: boolean;
  enableNewDashboardVersionNotification: boolean;
  connectionStateCheckIntervalMs: number;
  ephemeralZipExportToken: boolean;
  directorySync: boolean;
  promos: boolean;
  dashboardShowCashUsage: boolean;
  s3ExportIntegration: boolean;
  showFivetranSyncProgress: boolean;
  supportTicketForAllPlans: boolean;
  healthDeployProgress: boolean;
  componentSwitcherTreeEnabled: boolean;
  preAuthorizeStarterPlan: boolean;
  preAuthorizePaymentMethodChange: boolean;
} = {
  commandPaletteDeleteProjects: false,
  enableNewDashboardVersionNotification: false,
  connectionStateCheckIntervalMs: 2500,
  ephemeralZipExportToken: false,
  directorySync: false,
  promos: false,
  dashboardShowCashUsage: false,
  s3ExportIntegration: false,
  showFivetranSyncProgress: false,
  supportTicketForAllPlans: false,
  healthDeployProgress: false,
  componentSwitcherTreeEnabled: false,
  preAuthorizeStarterPlan: false,
  preAuthorizePaymentMethodChange: false,
};

export const flagDefaultsKebabCase = Object.entries(flagDefaults).reduce(
  (carry, [key, value]) => ({ ...carry, [kebabCase(key)]: value }),
  {} as { [key: string]: any },
);

// useLaunchDarkly is a thin wrapper on LaunchDarkly's react sdk which adds manual to flag keys.
// At some point, we can generate this file.
export function useLaunchDarkly() {
  return useFlags<typeof flagDefaults>();
}
