import { useCommandState } from "cmdk";
import { useContext } from "react";
import { KEYCAP_CLASSES, KeyboardShortcut } from "@ui/KeyboardShortcut";
import { cn } from "@ui/cn";
import { REMOTE_VALUE_PREFIX } from "./navigation";
import { PaletteCopyContext } from "./copy";

// Values of items whose Enter action is direct navigation but that can also
// be browsed (drilled into) with the modifier — project and deployment
// results.
function isBrowsableItemValue(value: string | undefined): boolean {
  return (
    value !== undefined &&
    (value.startsWith(`${REMOTE_VALUE_PREFIX}project:`) ||
      value.startsWith("deployment:") ||
      value.startsWith(`${REMOTE_VALUE_PREFIX}deployment:`))
  );
}

export function Footer({
  inSubPage,
  status,
}: {
  inSubPage: boolean;
  // Optional status published by the active page, shown in the right gutter.
  status?: React.ReactNode;
}) {
  // cmdk keeps its selection in sync with both keyboard focus and pointer
  // hover, so this covers "hovering or focusing" a browsable item.
  const selectedValue = useCommandState((state) => state.value);
  const copyAction = useContext(PaletteCopyContext)?.actionFor(selectedValue);
  return (
    <div className="-mx-1 flex flex-wrap items-center gap-x-4 gap-y-1 border-t px-3 pt-2 pb-1.5 text-xs text-content-tertiary select-none">
      <span className="flex items-center gap-1">
        <KeyboardShortcut value={["Up", "Down"]} className={KEYCAP_CLASSES} />
        Navigate
      </span>
      <span className="flex items-center gap-1">
        <KeyboardShortcut value={["Return"]} className={KEYCAP_CLASSES} />
        Select
      </span>
      {inSubPage && (
        <span className="flex items-center gap-1">
          <KeyboardShortcut value={["Left"]} className={KEYCAP_CLASSES} />
          Back
        </span>
      )}
      {isBrowsableItemValue(selectedValue) && (
        <span className="flex animate-fadeInFromLoading items-center gap-1">
          <KeyboardShortcut value={["Right"]} className={KEYCAP_CLASSES} />
          Browse
        </span>
      )}
      {copyAction && (
        <span className="flex animate-fadeInFromLoading items-center gap-1">
          <KeyboardShortcut
            value={["CtrlOrCmd", "C"]}
            className={KEYCAP_CLASSES}
          />
          Copy {copyAction.label}
        </span>
      )}
      {status && (
        <span className="ml-auto text-content-secondary">{status}</span>
      )}
      <span className={cn("flex items-center gap-1", !status && "ml-auto")}>
        <KeyboardShortcut value={["Esc"]} className={KEYCAP_CLASSES} />
        {inSubPage ? "Back" : "Close"}
      </span>
    </div>
  );
}
