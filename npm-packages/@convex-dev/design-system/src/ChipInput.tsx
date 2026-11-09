import {
  Combobox,
  ComboboxInput,
  ComboboxOption,
  ComboboxOptions,
  Description,
  Field,
  Label,
} from "@headlessui/react";
import { Chip } from "@ui/Chip";
import { cn } from "@ui/cn";
import { ReactNode, useRef } from "react";

export type ChipInputOption<T> = { value: T; label: ReactNode };

export function ChipInput<T extends string | number>({
  label,
  labelHidden = false,
  values,
  onChange,
  chipLabel,
  removeLabel,
  removeTip,
  chipClassName,
  query,
  onQueryChange,
  options,
  onEnterQuery,
  onBlur,
  placeholder,
  disabled = false,
  error,
  description,
}: {
  label: string;
  labelHidden?: boolean;
  /** The selected values, in the order their chips appear. */
  values: T[];
  onChange: (values: T[]) => void;
  chipLabel: (value: T) => ReactNode;
  /** The accessible name of a chip's remove button. */
  removeLabel: (value: T) => string;
  removeTip?: ReactNode;
  chipClassName?: string;
  query: string;
  onQueryChange: (query: string) => void;
  options: ChipInputOption<T>[];
  /** Called on Enter over a typed query with no option highlighted. */
  onEnterQuery?: (query: string) => void;
  onBlur?: () => void;
  /** Shown while no value is selected. */
  placeholder?: string;
  disabled?: boolean;
  /** Replaces the description and marks the field invalid. */
  error?: string;
  /** Shown beneath the field while there's no error to show instead. */
  description?: ReactNode;
}) {
  const inputRef = useRef<HTMLInputElement>(null);

  return (
    // Headless UI sets the input's `aria-describedby` from the `Description`s
    // in its `Field`, so the note beneath the input is one.
    <Field disabled={disabled} className="flex w-full flex-col gap-1">
      <Combobox
        multiple
        immediate
        value={values}
        onChange={onChange}
        disabled={disabled}
      >
        {({ open, activeIndex }) => (
          <>
            <Label
              className={cn(
                "text-left text-sm text-content-primary",
                labelHidden && "sr-only",
              )}
            >
              {label}
            </Label>
            {/* eslint-disable-next-line jsx-a11y/no-static-element-interactions -- The input is reachable from the keyboard on its own; this only widens its mouse target. */}
            <div
              onMouseDown={(event) => {
                // Once chips wrap, the input fills only the last row, so the
                // field's own padding and the space beside the chips focus it
                // too. Chips handle their own clicks.
                if (event.target === event.currentTarget && !disabled) {
                  event.preventDefault();
                  inputRef.current?.focus();
                }
              }}
              className={cn(
                "flex w-full cursor-text flex-wrap items-center gap-1.5 rounded-md border bg-background-secondary px-2 py-1.5 text-sm",
                error
                  ? "focus-within:border-content-error"
                  : "focus-within:border-border-selected",
                disabled &&
                  "cursor-not-allowed bg-background-tertiary text-content-secondary",
              )}
            >
              {values.map((value) =>
                disabled ? (
                  <Chip key={value} className={chipClassName}>
                    {chipLabel(value)}
                  </Chip>
                ) : (
                  <Chip
                    key={value}
                    onRemove={() => onChange(values.filter((v) => v !== value))}
                    removeLabel={removeLabel(value)}
                    removeTip={removeTip}
                    className={chipClassName}
                  >
                    {chipLabel(value)}
                  </Chip>
                ),
              )}
              <ComboboxInput
                ref={inputRef}
                // Set here rather than through Headless UI's `invalid` prop,
                // which only adds `data-invalid`.
                aria-invalid={error ? true : undefined}
                value={query}
                onChange={(event) => onQueryChange(event.target.value)}
                onKeyDown={(event) => {
                  if (event.nativeEvent.isComposing) {
                    return;
                  }
                  // Headless UI handles Enter on an active option, and blocks
                  // the form's own submit while the list is open.
                  if (
                    event.key === "Enter" &&
                    !(open && activeIndex !== null)
                  ) {
                    event.preventDefault();
                    const trimmed = query.trim();
                    if (trimmed === "") {
                      event.currentTarget.form?.requestSubmit();
                    } else {
                      onEnterQuery?.(trimmed);
                    }
                  }
                  if (
                    event.key === "Backspace" &&
                    query === "" &&
                    values.length > 0
                  ) {
                    onChange(values.slice(0, -1));
                  }
                }}
                onBlur={onBlur}
                placeholder={values.length === 0 ? placeholder : undefined}
                className="min-w-24 flex-1 bg-transparent p-0.5 placeholder-content-tertiary focus:outline-hidden disabled:cursor-not-allowed"
              />
            </div>
            {/* Rendered even when empty so the expanded combobox always has
              the listbox it points to. */}
            <ComboboxOptions
              anchor={{ to: "bottom start", gap: 4 }}
              modal={false}
              className="z-50 max-h-60 max-w-sm min-w-48 overflow-auto rounded-md border bg-background-secondary py-1 text-xs shadow-sm empty:invisible focus:outline-hidden"
            >
              {options.map((option) => (
                <ComboboxOption
                  key={option.value}
                  value={option.value}
                  className="cursor-pointer truncate px-2 py-1.5 text-content-primary select-none data-focus:bg-background-tertiary"
                >
                  {option.label}
                </ComboboxOption>
              ))}
            </ComboboxOptions>
            {error ? (
              <Description
                as="p"
                className="animate-fadeInFromLoading text-xs text-content-errorSecondary"
                role="alert"
              >
                {error}
              </Description>
            ) : (
              description && (
                <Description
                  as="p"
                  className="max-w-prose text-xs text-content-secondary"
                >
                  {description}
                </Description>
              )
            )}
          </>
        )}
      </Combobox>
    </Field>
  );
}
