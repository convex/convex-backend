import { useContext } from "react";
import { Combobox } from "@ui/Combobox";
import { Link } from "@ui/Link";
import { PuzzlePieceIcon } from "@common/elements/icons";
import { Tooltip } from "@ui/Tooltip";
import { NENT_APP_PLACEHOLDER, Nent, useNents } from "@common/lib/useNents";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";

export function NentSwitcherList({
  onChange,
  className,
}: {
  onChange?: (nent: string | null) => void;
  className?: string;
}) {
  const { nents: allNents, selectedNent, setSelectedNent } = useNents();
  const { deploymentsURI } = useContext(DeploymentInfoContext);
  if (!allNents || allNents.length <= 1) {
    return null;
  }
  const nents = allNents.filter((nent) => nent.name !== null);

  const mounted = nents.filter((nent) => nent.state === "active");
  const unmounted = nents.filter((nent) => nent.state !== "active");

  return (
    <div className={className}>
      <Combobox
        buttonProps={{
          tip: "Switch between components installed in this deployment.",
          tipSide: "right",
        }}
        label="Select component"
        selectedOption={selectedNent || undefined}
        buttonClasses="text-right truncate w-full"
        innerButtonClasses={
          selectedNent
            ? "bg-yellow-100/50 dark:bg-yellow-600/20 hover:bg-yellow-100 dark:hover:bg-yellow-600/50"
            : ""
        }
        className="static truncate"
        Option={NentNameOption}
        setSelectedOption={(nent) => {
          void setSelectedNent(nent?.id ?? undefined);
          onChange?.(nent?.id || null);
        }}
        searchPlaceholder="Search components..."
        options={[
          { label: NENT_APP_PLACEHOLDER, value: undefined },
          ...mounted.map((nent) => ({
            label: nent.path,
            value: nent,
          })),
          ...unmounted.map((nent) => ({
            label: nent.path,
            value: nent,
          })),
        ]}
      />
      {selectedNent && selectedNent.state !== "active" && (
        <p className="mt-1 text-xs text-content-secondary">
          This component is unmounted. You can delete it and its data on the{" "}
          <Link passHref href={`${deploymentsURI}/settings/components`}>
            Components settings
          </Link>{" "}
          page.
        </p>
      )}
    </div>
  );
}

export function NentNameOption({
  label,
  value,
  inButton,
}: {
  label: string;
  value?: Nent;
  inButton: boolean;
}) {
  return (
    <Tooltip
      tip={
        value && value.state !== "active"
          ? "This component is unmounted"
          : undefined
      }
      side="right"
      className="flex w-full items-center"
    >
      <div className="flex items-center gap-1 truncate">
        {inButton && <PuzzlePieceIcon className="mt-px min-w-[13px]" />}
        <span className="truncate">
          {label === NENT_APP_PLACEHOLDER ? "app" : label}
          {value && value.state !== "active" && "*"}
        </span>
      </div>
    </Tooltip>
  );
}
