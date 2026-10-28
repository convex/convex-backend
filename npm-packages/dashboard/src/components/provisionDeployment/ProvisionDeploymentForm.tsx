import { useEffect, useState, useRef, useId, useCallback } from "react";
import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { Tooltip } from "@ui/Tooltip";
import type { DeploymentRegionMetadata } from "@convex-dev/platform/managementApi";
import type { RegionName } from "generatedApi";
import { useCurrentTeam, useUpdateTeam } from "api/teams";
import { useIsCurrentMemberTeamAdmin } from "api/roles";
import { useRouter } from "next/router";
import { useProvisionDeployment } from "api/deployments";
import { Fieldset, Legend, RadioGroup } from "@headlessui/react";
import { cn } from "@ui/cn";
import { Sheet } from "@ui/Sheet";
import { useCurrentTheme } from "@common/lib/useCurrentTheme";
import createGlobe from "cobe";
import { SignalIcon } from "@heroicons/react/24/outline";
import { GlobeIcon } from "@radix-ui/react-icons";
import { Region } from "elements/Region";
import { RegionPricingWarning } from "elements/RegionPricingWarning";
import {
  DEFAULT_GLOBE_COORDINATES,
  getRegionCoordinates,
  useEnabledRegionNames,
  useSelectableRegions,
} from "lib/regions";

export function ProvisionDeploymentForm({
  projectId,
  projectURI,
  deploymentType,
}: {
  projectId: number;
  projectURI: string;
  deploymentType: "prod" | "dev";
}) {
  const router = useRouter();
  const team = useCurrentTeam();
  const provisionDeployment = useProvisionDeployment(projectId);
  const updateTeam = useUpdateTeam(team?.id ?? 0, /* toast */ false);
  const isAdmin = useIsCurrentMemberTeamAdmin();

  const { regions, expectedRegionCount } = useSelectableRegions(team?.id);

  const handleCreate = useCallback(
    async (region: string, setAsDefault: boolean) => {
      if (setAsDefault) {
        await updateTeam({ defaultRegion: region as RegionName });
      }
      const { name } = await provisionDeployment({
        type: deploymentType,
        region: region as RegionName,
      });
      void router.replace(`${projectURI}/${name}`);
    },
    [updateTeam, provisionDeployment, deploymentType, router, projectURI],
  );

  return (
    <ProvisionDeploymentFormInner
      deploymentType={deploymentType}
      regions={regions}
      expectedRegionCount={expectedRegionCount}
      onCreate={handleCreate}
      teamSlug={team?.slug}
      teamName={team?.name}
      isAdmin={isAdmin}
    />
  );
}

export function ProvisionDeploymentFormInner({
  deploymentType,
  regions,
  expectedRegionCount,
  onCreate,
  teamSlug,
  teamName,
  isAdmin,
}: {
  deploymentType: "prod" | "dev";
  regions: DeploymentRegionMetadata[] | undefined;
  /** How many region tiles to render while `regions` loads. */
  expectedRegionCount: number;
  onCreate: (region: string, setAsDefault: boolean) => Promise<void>;
  teamSlug: string | undefined;
  teamName: string | undefined;
  isAdmin: boolean;
}) {
  const [selectedRegion, setSelectedRegion] = useState<RegionName | null>(null);
  const [isCreating, setIsCreating] = useState(false);
  const [setAsDefault, setSetAsDefault] = useState(false);

  // When the user is an admin, default “set as default” to true
  useEffect(() => {
    setSetAsDefault(isAdmin);
  }, [isAdmin]);

  // Select the first region by default (will be us-east in prod)
  useEffect(() => {
    if (!selectedRegion && regions && regions.length > 0) {
      setSelectedRegion(regions[0].name);
    }
  }, [regions, selectedRegion]);

  const defaultCheckboxId = useId();

  return (
    <div className="flex size-full justify-center">
      <div className="my-auto flex w-full max-w-xl flex-col gap-6 p-4">
        <Sheet className="relative overflow-hidden">
          <Globe selectedRegion={selectedRegion} />
          <form
            className="relative flex flex-col gap-6 p-3"
            onSubmit={async (e: React.FormEvent<HTMLFormElement>) => {
              e.preventDefault();
              if (!selectedRegion) {
                return;
              }
              setIsCreating(true);
              try {
                await onCreate(selectedRegion, setAsDefault);
              } catch (error) {
                setIsCreating(false);
                throw error;
              }
            }}
          >
            <h3 className="flex flex-col gap-0.5">
              <span>Create a new deployment</span>
              <span
                className={cn(
                  "inline-flex items-center gap-1.5",
                  deploymentType === "prod"
                    ? "text-purple-600 dark:text-purple-100"
                    : "text-green-600 dark:text-green-400",
                )}
              >
                {deploymentType === "prod" ? (
                  <SignalIcon className="size-4 shrink-0" />
                ) : (
                  <GlobeIcon className="size-4 shrink-0" />
                )}
                {deploymentType === "prod" ? "Production" : "Development"}
              </span>
            </h3>
            <Fieldset>
              <Legend className="mb-1 text-sm text-content-primary">
                Region
              </Legend>
              <RadioGroup
                name="region"
                value={selectedRegion ?? null}
                onChange={setSelectedRegion}
              >
                <div className="grid auto-rows-fr grid-cols-1 gap-4 sm:grid-cols-2">
                  {regions === undefined
                    ? Array.from({ length: expectedRegionCount }, (_, i) => (
                        <Region
                          key={i}
                          region={undefined}
                          teamSlug={teamSlug}
                        />
                      ))
                    : regions.map((region) => (
                        <Region
                          key={region.name}
                          region={region}
                          teamSlug={teamSlug}
                        />
                      ))}
                </div>
              </RadioGroup>
              <RegionPricingWarning region={selectedRegion} />
            </Fieldset>

            <Tooltip
              tip={
                isAdmin
                  ? undefined
                  : "You do not have permission to update the region for new deployments."
              }
            >
              <label
                htmlFor={defaultCheckboxId}
                className={cn(
                  "flex items-start gap-2 text-sm",
                  !isAdmin && "cursor-not-allowed opacity-50",
                )}
              >
                {/* align with the first line of the paragraph */}
                <div className="mt-[0.2rem] flex">
                  <Checkbox
                    id={defaultCheckboxId}
                    checked={setAsDefault}
                    onChange={() => setSetAsDefault(!setAsDefault)}
                    disabled={!isAdmin}
                  />
                </div>
                <p className="mt-0 text-left">
                  Use this region for all new deployments in{" "}
                  <span className="font-medium">{teamName}</span>
                </p>
              </label>
            </Tooltip>

            <div>
              <Button
                type="submit"
                disabled={!selectedRegion}
                loading={isCreating}
              >
                Create deployment
              </Button>
            </div>
          </form>
        </Sheet>
      </div>
    </div>
  );
}

// cobe renders into a canvas `devicePixelRatio` times the canvas's CSS size,
// but positions the globe in a fixed `GLOBE_SIZE` coordinate space.
const GLOBE_DEVICE_PIXEL_RATIO = 2;
const GLOBE_SIZE = 900;
// cobe draws the globe with this radius, in units of half `GLOBE_SIZE / scale`.
const GLOBE_RADIUS = 0.8;

function Globe({ selectedRegion }: { selectedRegion: RegionName | null }) {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const markerRefs = useRef(new Map<RegionName, HTMLDivElement>());
  const focusRef = useRef<[number, number]>([0, 0]);
  const currentTheme = useCurrentTheme();
  const isDark = currentTheme === "dark";

  // Derived from the launch flags rather than the fetched region list, so the
  // globe isn't torn down and rebuilt when that request lands.
  const regionNames = useEnabledRegionNames();

  // Update focus when region changes
  useEffect(() => {
    const coordinates = selectedRegion && getRegionCoordinates(selectedRegion);
    if (coordinates) {
      focusRef.current = locationToAngles(...coordinates);
    }
  }, [selectedRegion]);

  useEffect(() => {
    if (!canvasRef.current) return;

    let windowWidth = 0;
    focusRef.current = locationToAngles(...DEFAULT_GLOBE_COORDINATES);
    let [currentPhi, currentTheta] = [...focusRef.current];
    const doublePi = Math.PI * 2;

    const onResize = () => {
      if (canvasRef.current) {
        windowWidth = window.innerWidth;
      }
    };
    window.addEventListener("resize", onResize);
    onResize();

    const globe = createGlobe(canvasRef.current, {
      devicePixelRatio: GLOBE_DEVICE_PIXEL_RATIO,
      width: 0,
      height: 0,
      scale: 0,
      phi: currentPhi,
      theta: currentTheta,
      dark: 0,
      diffuse: isDark ? 3 : 7,
      mapSamples: 20000,
      mapBrightness: isDark ? 6 : 4,
      baseColor: isDark ? [0.3, 0.3, 0.3] : [1, 1, 1],
      markerColor: [0.5, 0.5, 0.5],
      glowColor: isDark
        ? [42 / 255, 40 / 255, 37 / 255]
        : [253 / 255, 252 / 255, 250 / 255],
      // The region markers are DOM elements laid over the canvas, so they can
      // use the design system's colors and CSS transitions.
      markers: [],
      onRender: (state) => {
        state.phi = currentPhi;
        state.theta = currentTheta;

        const sm = windowWidth >= 640; // from Tailwind
        const offset: [number, number] = sm ? [900, -320] : [500, -410];
        const scale = sm ? 1.15 : 1.1;
        state.width = GLOBE_SIZE;
        state.height = GLOBE_SIZE;
        state.offset = offset;
        state.scale = scale;
        state.mapSamples = sm ? 25000 : 20000;

        const canvasHeight = canvasRef.current?.clientHeight ?? 0;
        for (const name of regionNames) {
          const marker = markerRefs.current.get(name);
          const coordinates = getRegionCoordinates(name);
          if (!marker || !coordinates) continue;
          const { x, y, depth } = projectLocation(
            coordinates,
            currentPhi,
            currentTheta,
            scale,
            offset,
            canvasHeight,
          );
          marker.style.transform = `translate(${x}px, ${y}px)`;
          // Fade markers out toward the edge of the globe, like its land dots.
          marker.style.opacity = String(Math.min(Math.max(depth * 2, 0), 1));
        }

        const [focusPhi, focusTheta] = focusRef.current;
        const distPositive = (focusPhi - currentPhi + doublePi) % doublePi;
        const distNegative = (currentPhi - focusPhi + doublePi) % doublePi;

        const speed = 0.03;

        if (distPositive < distNegative) {
          currentPhi += distPositive * speed;
        } else {
          currentPhi -= distNegative * speed;
        }
        currentTheta = currentTheta * (1 - speed) + focusTheta * speed;
      },
    });

    setTimeout(() => {
      if (canvasRef.current) {
        canvasRef.current.style.opacity = "1";
      }
    });

    return () => {
      globe.destroy();
      window.removeEventListener("resize", onResize);
    };
  }, [isDark, regionNames]);

  return (
    <div
      className="pointer-events-none absolute inset-0 size-full overflow-hidden"
      aria-hidden
    >
      <canvas
        className="absolute inset-0 size-full"
        ref={canvasRef}
        style={{
          opacity: 0,
          transition: "opacity 1s ease",
        }}
      />
      {regionNames.map((name) => {
        const isSelected = name === selectedRegion;
        return (
          <div
            key={name}
            ref={(element) => {
              if (element) {
                markerRefs.current.set(name, element);
              } else {
                markerRefs.current.delete(name);
              }
            }}
            className="absolute top-0 left-0"
            style={{ opacity: 0 }}
          >
            <div className="relative flex -translate-1/2 items-center justify-center">
              <span
                className={cn(
                  "absolute size-6 rounded-full bg-util-accent/30 transition-[scale,opacity] duration-500",
                  isSelected ? "scale-100 opacity-100" : "scale-0 opacity-0",
                )}
              />
              <span
                className={cn(
                  "relative rounded-full ring-2 ring-background-secondary transition-[width,height,background-color] duration-500",
                  isSelected
                    ? "size-3 bg-util-accent"
                    : "size-2 bg-content-secondary",
                )}
              />
            </div>
          </div>
        );
      })}
    </div>
  );
}

function locationToAngles(lat: number, long: number): [number, number] {
  return [
    Math.PI - ((long * Math.PI) / 180 - Math.PI / 2),
    (lat * Math.PI) / 180,
  ];
}

/**
 * Projects a location to CSS pixels from the top-left of the globe's canvas,
 * matching cobe 0.6.5's shader. `depth` is positive on the visible hemisphere.
 */
function projectLocation(
  [lat, long]: [number, number],
  phi: number,
  theta: number,
  scale: number,
  [offsetX, offsetY]: [number, number],
  canvasHeight: number,
): { x: number; y: number; depth: number } {
  const latRad = (lat * Math.PI) / 180;
  const longRad = (long * Math.PI) / 180 - Math.PI;
  const cosLat = Math.cos(latRad);
  const px = -cosLat * Math.cos(longRad);
  const py = Math.sin(latRad);
  const pz = cosLat * Math.sin(longRad);

  const cosTheta = Math.cos(theta);
  const sinTheta = Math.sin(theta);
  const cosPhi = Math.cos(phi);
  const sinPhi = Math.sin(phi);
  const rotatedX = cosPhi * px + sinPhi * pz;
  const rotatedY =
    sinPhi * sinTheta * px + cosTheta * py - cosPhi * sinTheta * pz;
  const depth =
    -sinPhi * cosTheta * px + sinTheta * py + cosPhi * cosTheta * pz;

  // Inverts the shader's mapping from fragment coordinates (device pixels,
  // origin at the bottom left) to the globe's coordinate space.
  const fragX =
    (GLOBE_SIZE / 2) *
    (scale * (GLOBE_RADIUS * rotatedX + offsetX / GLOBE_SIZE) + 1);
  const fragY =
    (GLOBE_SIZE / 2) *
    (scale * (GLOBE_RADIUS * rotatedY - offsetY / GLOBE_SIZE) + 1);
  return {
    x: fragX / GLOBE_DEVICE_PIXEL_RATIO,
    y: canvasHeight - fragY / GLOBE_DEVICE_PIXEL_RATIO,
    depth,
  };
}
