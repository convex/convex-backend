import { ValidatorJSON, Value } from "convex/values";
import { useState } from "react";
import isEqual from "lodash/isEqual";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import { ObjectEditor } from "@common/elements/ObjectEditor/ObjectEditor";
import { KEYCAP_CLASSES, KeyboardShortcut } from "@ui/KeyboardShortcut";
import { useTableDensity } from "@common/features/data/lib/useTableDensity";
import { DateTimePicker } from "@common/features/data/components/DateTimePicker";
import { isInCommonUTCTimestampRange } from "@common/features/data/lib/helpers";

export const CELL_EDITOR_OVERHANG = 2;

export type CellEditorProps = {
  value?: Value;
  defaultValue?: Value;
  onStopEditing: () => void;
  onSave(value?: Value): Promise<any>;
  validator?: ValidatorJSON;
  shouldSurfaceValidatorErrors?: boolean;
  allowTopLevelUndefined?: boolean;
  // Whether the column this cell belongs to is displayed as dates. Controls
  // whether the editor opens on the date picker or the raw number by default.
  inferIsDate?: boolean;
};

export function CellEditor({
  value,
  defaultValue,
  onStopEditing,
  onSave,
  validator,
  shouldSurfaceValidatorErrors,
  allowTopLevelUndefined,
  inferIsDate = true,
}: CellEditorProps) {
  const [path] = useState(Math.random());
  const [error, setError] = useState<string | undefined>(undefined);

  const saveEditedValue = async (editedValue?: Value) => {
    if (editedValue === undefined || error) {
      return;
    }

    onStopEditing();
    if (isEqual(value, editedValue)) {
      return;
    }
    await onSave(editedValue);
  };

  const [editedValue, setEditedValue] = useState(
    defaultValue === undefined ? value : defaultValue,
  );

  const [wasInCommonUTCTimestampRange] = useState(
    typeof editedValue === "number" && isInCommonUTCTimestampRange(editedValue),
  );

  const isTimestampLike =
    typeof editedValue === "number" && wasInCommonUTCTimestampRange;

  const [showAsDate, setShowAsDate] = useState(isTimestampLike && inferIsDate);

  const [innerText, setInnerText] = useState<string | undefined>(undefined);
  const { densityValues } = useTableDensity();

  return (
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions
    <div
      className="flex size-full flex-col rounded-lg border border-border-selected bg-background-secondary text-xs text-content-primary shadow-lg"
      onKeyDown={(e) => {
        if (isTimestampLike && e.ctrlKey && e.shiftKey && e.code === "KeyD") {
          e.preventDefault();
          setShowAsDate(!showAsDate);
        }
      }}
    >
      {/* Monaco editor cannot show a placeholder, so render our own. */}
      {!innerText && editedValue === UNDEFINED_PLACEHOLDER && !error && (
        <div
          className="pointer-events-none absolute z-50 font-mono text-xs text-content-secondary italic"
          data-testid="undefined-placeholder"
          style={{
            top: densityValues.paddingY + 2 + CELL_EDITOR_OVERHANG,
            left: densityValues.paddingX + CELL_EDITOR_OVERHANG,
          }}
        >
          unset
        </div>
      )}
      <div
        className="flex min-h-0 flex-1 flex-col"
        style={{
          paddingLeft: densityValues.paddingX + CELL_EDITOR_OVERHANG,
          paddingRight: densityValues.paddingX + CELL_EDITOR_OVERHANG,
          paddingTop: densityValues.paddingY + CELL_EDITOR_OVERHANG,
        }}
      >
        {showAsDate && isTimestampLike && typeof editedValue === "number" ? (
          <DateTimePicker
            date={new Date(editedValue)}
            onChange={(date) => setEditedValue(date.getTime())}
            onSave={() => saveEditedValue(editedValue)}
            className="-ml-px font-mono"
            autoFocus
            aria-label="Edit timestamp as date and time"
          />
        ) : (
          <ObjectEditor
            validator={validator}
            shouldSurfaceValidatorErrors={shouldSurfaceValidatorErrors}
            padding={false}
            autoFocus
            enterSaves
            saveAction={() => saveEditedValue(editedValue)}
            disableFind
            defaultValue={
              defaultValue === UNDEFINED_PLACEHOLDER ? undefined : editedValue
            }
            onChange={setEditedValue}
            onChangeInnerText={setInnerText}
            onError={(errors) =>
              setError(errors.length > 0 ? errors[0] : undefined)
            }
            path={path.toString()}
            disableFolding
            className="border-none"
            allowTopLevelUndefined={allowTopLevelUndefined}
            mode="editField"
            fixedOverflowWidgets={false}
          />
        )}
      </div>
      <div className="flex shrink-0 flex-wrap items-center gap-4 px-2 pt-1 pb-1.5 text-content-secondary">
        {isTimestampLike && (
          <span className="flex min-w-fit items-center gap-1">
            <KeyboardShortcut
              value={["Ctrl", "Shift", "D"]}
              className={KEYCAP_CLASSES}
            />
            to show as {showAsDate ? "number" : "date"}
          </span>
        )}
        {error ? (
          <p
            className="ml-auto font-mono break-all text-content-errorSecondary"
            role="alert"
          >
            {`${error.slice(0, 80)}${error.length > 80 ? "..." : ""}`}
          </p>
        ) : (
          <>
            <span className="ml-auto flex items-center gap-1">
              <KeyboardShortcut value={["Esc"]} className={KEYCAP_CLASSES} />
              to cancel
            </span>
            <span className="flex items-center gap-1">
              <KeyboardShortcut value={["Return"]} className={KEYCAP_CLASSES} />
              to save
            </span>
          </>
        )}
      </div>
    </div>
  );
}
