import classNames from "classnames";
import { forwardRef, useId } from "react";
import { Checkbox } from "@ui/Checkbox";
import { Tooltip } from "@ui/Tooltip";
import { useTableDensity } from "../../lib/useTableDensity";

type TableCheckboxProps = {
  checked: boolean;
  isSelectionAllNonExhaustive?: boolean;
  onToggle(): void;
  onToggleAdjacent(): void;
  className?: string;
  width?: string;
  onKeyDown?: (event: React.KeyboardEvent<HTMLInputElement>) => void;
};

export const TableCheckbox = forwardRef<HTMLLabelElement, TableCheckboxProps>(
  function TableCheckbox(
    {
      checked,
      isSelectionAllNonExhaustive = false,
      onToggle,
      onToggleAdjacent,
      onKeyDown,
      className = undefined,
      width,
    },
    ref,
  ) {
    const id = useId();

    const { densityValues } = useTableDensity();

    const label = (
      <label
        ref={ref}
        htmlFor={id}
        aria-label="Select row or column"
        className={classNames(
          "flex items-center justify-center h-full",
          isSelectionAllNonExhaustive ? "cursor-not-allowed" : "cursor-pointer",
          className,
        )}
        style={{
          width,
          height: densityValues.height,
        }}
      >
        <Checkbox
          id={id}
          disabled={isSelectionAllNonExhaustive}
          onKeyDown={onKeyDown}
          onChange={(event) => {
            // @ts-expect-error shiftKey will exist on change events triggered by the mouse
            if (event.nativeEvent.shiftKey) {
              onToggleAdjacent();
            } else {
              onToggle();
            }
          }}
          checked={checked}
        />
      </label>
    );

    // Only wrap in a tooltip when the checkbox is disabled, so we can explain
    // why an individual document can't be deselected. Wrapping the enabled
    // checkbox would suppress the label's native "click anywhere in the cell to
    // toggle" behavior and pop a tip on every row hover.
    if (!isSelectionAllNonExhaustive) {
      return label;
    }

    return (
      <Tooltip
        asChild
        side="right"
        tip="Every document is selected, so edits apply across all documents in this table. Individual documents can't be deselected while all are selected. Use the checkbox in the header to clear the selection."
      >
        {label}
      </Tooltip>
    );
  },
);
