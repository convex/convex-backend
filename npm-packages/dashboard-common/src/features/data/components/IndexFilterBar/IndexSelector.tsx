import { Button } from "@ui/Button";
import { Combobox } from "@ui/Combobox";
import { IndexIcon } from "@common/elements/icons";
import {
  DatabaseIndexDef,
  IndexDef,
  SearchIndexDef,
  databaseIndexDefs,
  searchIndexDefs,
} from "./filterModel";

// The index name at the head of the filter path doubles as the index
// picker. Every table has at least the two system indexes, so the path
// always starts here.
export function IndexSelector({
  indexDefs,
  current,
  onChooseIndex,
  onChooseSearch,
}: {
  indexDefs: IndexDef[];
  current: IndexDef;
  onChooseIndex(def: DatabaseIndexDef): void;
  onChooseSearch(def: SearchIndexDef): void;
}) {
  const options = [
    ...databaseIndexDefs(indexDefs),
    ...searchIndexDefs(indexDefs),
  ].map((def) => ({ value: def, label: def.name }));
  return (
    <Combobox
      label="Index"
      size="sm"
      optionsWidth="fit"
      searchPlaceholder="Search indexes..."
      options={options}
      selectedOption={current}
      setSelectedOption={(def) => {
        if (!def || def.name === current.name) return;
        if (def.kind === "search") {
          onChooseSearch(def);
        } else {
          onChooseIndex(def);
        }
      }}
      Option={IndexOption}
      buttonClasses="w-fit"
      innerButtonClasses="h-6 px-1.5 font-mono"
      buttonProps={{
        tipSide: "bottom",
      }}
    />
  );
}

function IndexOption({
  label,
  value,
  inButton,
}: {
  label: string;
  value: IndexDef;
  inButton: boolean;
}) {
  const fields = value.kind === "search" ? [value.searchField] : value.fields;
  return (
    <div className="flex items-center gap-2 text-xs">
      <IndexIcon
        kind={value.kind}
        className={inButton ? "text-content-primary" : "text-content-tertiary"}
      />
      <div>
        <div className="font-mono">{label}</div>
        {!inButton && (
          <div className="text-xs text-content-secondary">
            ({fields.join(", ")})
          </div>
        )}
      </div>
    </div>
  );
}

// A→Z / Z→A glyph: the letters say which way the order runs, the arrow says
// it is a sort. The letters slide past each other when the order flips so
// the change registers.
function SortGlyph({ desc }: { desc: boolean }) {
  const letterClasses =
    "transition-transform duration-300 ease-out motion-reduce:transition-none";
  const letterProps = {
    x: 0.5,
    y: 7,
    fontSize: 7.5,
    fontWeight: 700,
    fill: "currentColor",
    stroke: "none",
    fontFamily: "ui-sans-serif, system-ui, sans-serif",
  };
  return (
    <svg
      viewBox="0 0 16 16"
      className="size-4"
      aria-hidden
      fill="none"
      stroke="currentColor"
    >
      <text
        {...letterProps}
        className={letterClasses}
        style={{ transform: `translateY(${desc ? 8.5 : 0}px)` }}
      >
        A
      </text>
      <text
        {...letterProps}
        className={letterClasses}
        style={{ transform: `translateY(${desc ? 0 : 8.5}px)` }}
      >
        Z
      </text>
      <path
        d="M12 2.5v11M9.5 11l2.5 2.5L14.5 11"
        strokeWidth="1.4"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

export function OrderToggle({
  order,
  sortField,
  indexName,
  disabled,
  onChange,
}: {
  order: "asc" | "desc";
  sortField: string;
  indexName: string;
  disabled: boolean;
  onChange(order: "asc" | "desc"): void;
}) {
  const label = order === "desc" ? "Descending" : "Ascending";
  return (
    <Button
      size="xs"
      variant="neutral"
      className={TOOLBAR_ICON_BUTTON_CLASSES}
      disabled={disabled}
      onClick={() => onChange(order === "asc" ? "desc" : "asc")}
      icon={<SortGlyph desc={order === "desc"} />}
      tip={
        disabled
          ? undefined
          : `${label} by ${sortField} using index ${indexName}. Click to flip the order.`
      }
      tipSide="bottom"
      aria-label={`Sort order: ${label.toLowerCase()}`}
    />
  );
}

// Matches the "Show or hide fields" button so the toolbar's icon buttons
// line up.
export const TOOLBAR_ICON_BUTTON_CLASSES =
  "h-[27.5px] w-fit min-w-[27.5px] justify-center rounded-lg p-1 text-xs";
