import { Tooltip } from "@ui/Tooltip";
import { PuzzlePieceIcon } from "@common/elements/icons";
import { NENT_APP_PLACEHOLDER, Nent } from "@common/lib/useNents";
import { NentSwitcherTree } from "@common/elements/NentSwitcherTree";

export function NentSwitcher(props: {
  onChange?: (nent: string | null) => void;
  className?: string;
}) {
  return <NentSwitcherTree {...props} />;
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
