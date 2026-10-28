import { DeploymentRegionMetadata } from "@convex-dev/platform/managementApi";
import { Field, Radio, Label } from "@headlessui/react";
import { cn } from "@ui/cn";
import { Tooltip } from "@ui/Tooltip";
import { Link } from "@ui/Link";
import { Loading } from "@ui/Loading";
import { permissionDeniedTip } from "elements/permissionDeniedTip";
import { getRegionFlag } from "lib/regions";

export function Region({
  region,
  teamSlug,
  disabledDueToPermissions = false,
}: {
  region: DeploymentRegionMetadata | null | undefined; // undefined = loading, null = ask every time
  teamSlug: string | undefined;
  disabledDueToPermissions?: boolean;
}) {
  if (region === undefined) {
    return (
      <Loading
        className="h-full min-h-[60px] rounded-xl border bg-background-secondary"
        fullHeight={false}
      />
    );
  }

  const isAskEveryTime = region === null;
  const disabledDueToAvailability = !isAskEveryTime && !region.available;
  const disabled = disabledDueToPermissions || disabledDueToAvailability;
  const value = region?.name ?? null;
  const { mainName, placeName } = isAskEveryTime
    ? { mainName: "Ask every time", placeName: "" }
    : parseRegionLabel(region.displayName);

  const content = (
    <div className="flex size-full text-start">
      <Field disabled={disabled} className="flex-1">
        <Radio
          value={value}
          className={cn(
            "group relative flex cursor-pointer rounded-xl border px-4 py-3 focus:outline-none sm:flex sm:justify-between",
            "h-full",
            "[--region-selector-border:transparent]",
            "data-checked:[--region-selector-border:var(--border-selected)]",
            "data-focus:[--region-selector-border:var(--border-selected)]",
            "bg-background-secondary transition-colors hover:bg-background-primary",
            "data-disabled:cursor-not-allowed data-disabled:bg-background-tertiary",
          )}
        >
          <span
            className={cn(
              "border border-(--region-selector-border)",
              "pointer-events-none absolute -inset-px rounded-xl",
            )}
            aria-hidden="true"
          />
          <div className="flex w-full items-center justify-between">
            <div className="flex items-center gap-3">
              {!isAskEveryTime && (
                <span
                  // eslint-disable-next-line no-restricted-syntax
                  className="text-xl leading-none"
                  role="presentation"
                >
                  {getRegionFlag(region.name)}
                </span>
              )}
              <div className="flex flex-col gap-0.5">
                <Label
                  className={cn(
                    "text-sm/tight font-semibold",
                    disabled
                      ? "text-content-secondary"
                      : "text-content-primary",
                  )}
                >
                  {mainName}
                </Label>
                {placeName && (
                  <span className="text-xs/tight text-content-secondary">
                    {placeName}
                  </span>
                )}
              </div>
            </div>
            <div className="ml-4 flex items-center gap-3">
              <div className="flex size-3.5 items-center justify-center rounded-full border border-border-transparent bg-background-secondary group-data-checked:border-util-accent group-data-checked:bg-util-accent">
                <span className="invisible size-1.5 rounded-full bg-white group-data-checked:visible" />
              </div>
            </div>
          </div>
        </Radio>
      </Field>
    </div>
  );

  if (disabledDueToPermissions) {
    return (
      <Tooltip
        className="w-full"
        tip={permissionDeniedTip(
          "You do not have permission to update the region for new deployments.",
          "team:update",
        )}
        side="top"
      >
        {content}
      </Tooltip>
    );
  }

  if (disabledDueToAvailability) {
    return (
      <Tooltip
        className="w-full"
        tip={
          <div className="flex flex-col gap-1">
            <p>This region is not available on your current plan.</p>
            {teamSlug && (
              <p>
                <Link
                  href={`/t/${teamSlug}/settings/billing`}
                  onClick={(e) => e.stopPropagation()}
                >
                  Upgrade your plan
                </Link>{" "}
                to access this region.
              </p>
            )}
          </div>
        }
        side="top"
      >
        {content}
      </Tooltip>
    );
  }

  return content;
}

function parseRegionLabel(label: string): {
  mainName: string;
  placeName: string;
} {
  const match = label.match(/^(.+?)\s*\((.+?)\)$/);
  if (match) {
    return { mainName: match[1].trim(), placeName: match[2].trim() };
  }
  return { mainName: label, placeName: "" };
}
