import { useMemo } from "react";
import type { RegionName } from "generatedApi";
import type { DeploymentRegionMetadata } from "@convex-dev/platform/managementApi";
import { useDeploymentRegions } from "api/deployments";
import { flagDefaults, useLaunchDarkly } from "hooks/useLaunchDarkly";

/** Default region for the globe and the base regional price. */
export const PRIMARY_REGION: RegionName = "aws-us-east-1";

type RegionAvailabilityFlag = {
  [K in keyof typeof flagDefaults]: (typeof flagDefaults)[K] extends boolean
    ? K
    : never;
}[keyof typeof flagDefaults];

type RegionPresentation = {
  flag: string;
  /** [latitude, longitude] of the region's marker on the globe. */
  coordinates: [number, number];
  /**
   * LaunchDarkly flag that must be on before the region is offered as a choice.
   * Regions without one are generally available.
   */
  availabilityFlag?: RegionAvailabilityFlag;
};

/**
 * Presentation details for each region, keyed by the name Big Brain returns.
 * Iteration order is the order regions are offered in, so keep the generally
 * available ones first, and update `REGION_ORDER` in the CLI's
 * `npm-packages/convex/src/cli/lib/utils/utils.ts` to match.
 */
export const REGION_PRESENTATION: Record<RegionName, RegionPresentation> = {
  "aws-us-east-1": { flag: "🇺🇸", coordinates: [38.9072, -77.0369] }, // Washington DC
  "aws-eu-west-1": { flag: "🇪🇺", coordinates: [53.3498, -6.2603] }, // Dublin
  "aws-ca-central-1": { flag: "🇨🇦", coordinates: [45.5019, -73.5674] }, // Montréal
  "aws-ap-southeast-2": { flag: "🇦🇺", coordinates: [-33.8688, 151.2093] }, // Sydney
};

const REGION_ORDER = Object.keys(REGION_PRESENTATION);

/** Where the globe points before a region is selected. */
export const DEFAULT_GLOBE_COORDINATES =
  REGION_PRESENTATION[PRIMARY_REGION].coordinates;

export function getRegionFlag(regionName: string): string {
  return REGION_PRESENTATION[regionName as RegionName]?.flag ?? "🏳️";
}

export function getRegionCoordinates(
  regionName: string,
): [number, number] | undefined {
  return REGION_PRESENTATION[regionName as RegionName]?.coordinates;
}

/**
 * Sorts known regions into {@link REGION_ORDER}, leaving regions this build
 * doesn't know about (a newly launched one, or `local` in dev) in server order
 * at the end.
 */
export function sortRegions<T extends { name: string }>(regions: T[]): T[] {
  return [...regions].sort((a, b) => {
    const aIndex = REGION_ORDER.indexOf(a.name);
    const bIndex = REGION_ORDER.indexOf(b.name);
    if (aIndex !== -1 && bIndex !== -1) return aIndex - bIndex;
    if (aIndex !== -1) return -1;
    if (bIndex !== -1) return 1;
    return 0;
  });
}

/**
 * The regions this dashboard offers, in display order. Regions that haven't
 * launched yet are hidden until their flag is on.
 */
export function useEnabledRegionNames(): RegionName[] {
  // `useFlags` hands back the object straight out of context, so its identity
  // is stable until a flag actually changes.
  const flags = useLaunchDarkly();

  return useMemo(
    () =>
      (Object.keys(REGION_PRESENTATION) as RegionName[]).filter((name) => {
        const { availabilityFlag } = REGION_PRESENTATION[name];
        // `flagDefaults` is hand-written, so LaunchDarkly can serve a
        // non-boolean; `=== true` hides the region rather than showing it.
        return (
          availabilityFlag === undefined || flags[availabilityFlag] === true
        );
      }),
    [flags],
  );
}

/**
 * The regions a team may create a deployment in, sorted for display and with
 * unlaunched regions hidden.
 *
 * Deployments can live in a region that is hidden here, so look region names up
 * against {@link useDeploymentRegions} when displaying an existing deployment.
 */
export function useSelectableRegions(teamId: number | undefined): {
  regions: DeploymentRegionMetadata[] | undefined;
  /** How many regions to expect, for rendering loading placeholders. */
  expectedRegionCount: number;
} {
  const { regions } = useDeploymentRegions(teamId);
  const enabledNames = useEnabledRegionNames();

  return {
    regions: useMemo(
      () =>
        regions
          ? sortRegions(
              regions.filter(
                (region) =>
                  // Regions Big Brain knows about but this build doesn't are
                  // left visible, so launching one doesn't need a dashboard
                  // deploy.
                  !(region.name in REGION_PRESENTATION) ||
                  enabledNames.includes(region.name as RegionName),
              ),
            )
          : undefined,
      [regions, enabledNames],
    ),
    expectedRegionCount: enabledNames.length,
  };
}
