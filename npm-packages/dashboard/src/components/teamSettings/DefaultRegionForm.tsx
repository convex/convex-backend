import { Sheet } from "@ui/Sheet";
import { Link } from "@ui/Link";
import { useState } from "react";
import { RegionName, TeamResponse } from "generatedApi";
import { useSelectableRegions } from "lib/regions";
import { DefaultRegionSelector } from "./DefaultRegionSelector";
import { TEAM_SETTINGS_SECTIONS } from "lib/sectionAnchors";

export type DefaultRegionFormProps = {
  team: TeamResponse;
  onUpdateTeam: (body: { defaultRegion: RegionName | null }) => Promise<void>;
  canUpdate: boolean;
};

export function DefaultRegionForm({
  team,
  onUpdateTeam,
  canUpdate,
}: DefaultRegionFormProps) {
  const { regions, expectedRegionCount } = useSelectableRegions(team.id);
  // Track the selection locally so it updates immediately on click, while the
  // team data refetches in the background.
  const [selectedRegion, setSelectedRegion] = useState<RegionName | null>(
    team.defaultRegion ?? null,
  );

  return (
    <Sheet id={TEAM_SETTINGS_SECTIONS.defaultRegion.id} className="text-sm">
      <h3 className="mb-1">Default Region</h3>
      <p className="mb-4 max-w-prose text-content-secondary">
        The region where new deployments in this team are created. Usage in
        regions outside the US has a 30% regional surcharge.{" "}
        <Link
          href="https://docs.convex.dev/production/regions#pricing"
          target="_blank"
        >
          Learn more about regions
        </Link>
        .
      </p>
      <DefaultRegionSelector
        value={selectedRegion}
        onChange={(region) => {
          setSelectedRegion(region);
          void onUpdateTeam({ defaultRegion: region });
        }}
        regions={regions}
        expectedRegionCount={expectedRegionCount}
        teamSlug={team.slug}
        disabledDueToPermissions={!canUpdate}
      />
    </Sheet>
  );
}
