import { Fieldset, RadioGroup } from "@headlessui/react";
import { Region } from "elements/Region";
import {
  DeploymentRegionMetadata,
  RegionName,
} from "@convex-dev/platform/managementApi";
import { RegionPricingWarning } from "elements/RegionPricingWarning";

export function DefaultRegionSelector({
  value,
  onChange,
  regions,
  expectedRegionCount,
  teamSlug,
  disabledDueToPermissions = false,
}: {
  value: RegionName | null;
  onChange: (region: RegionName | null) => void;
  regions: DeploymentRegionMetadata[] | undefined;
  /** How many region tiles to render while `regions` loads. */
  expectedRegionCount: number;
  teamSlug: string | undefined;
  disabledDueToPermissions?: boolean;
}) {
  return (
    <Fieldset>
      <RadioGroup
        name="defaultRegion"
        aria-label="Default region"
        value={value}
        onChange={onChange}
      >
        <div className="grid auto-rows-fr grid-cols-[repeat(auto-fill,minmax(11rem,1fr))] gap-3">
          {regions ? (
            <>
              <Region
                region={null}
                teamSlug={teamSlug}
                disabledDueToPermissions={disabledDueToPermissions}
              />
              {regions.map((region) => (
                <Region
                  key={region.name}
                  region={region}
                  teamSlug={teamSlug}
                  disabledDueToPermissions={disabledDueToPermissions}
                />
              ))}
            </>
          ) : (
            // One extra tile for the "Ask every time" option.
            Array.from({ length: expectedRegionCount + 1 }, (_, i) => (
              <Region
                key={i}
                region={undefined}
                teamSlug={teamSlug}
                disabledDueToPermissions={disabledDueToPermissions}
              />
            ))
          )}
        </div>
      </RadioGroup>
      <RegionPricingWarning region={value} />
    </Fieldset>
  );
}
