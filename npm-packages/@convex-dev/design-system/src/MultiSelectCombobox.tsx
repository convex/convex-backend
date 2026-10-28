import {
  CheckIcon,
  ChevronDownIcon,
  MagnifyingGlassIcon,
} from "@radix-ui/react-icons";
import {
  Combobox as HeadlessCombobox,
  ComboboxButton as HeadlessComboboxButton,
  ComboboxOptions as HeadlessComboboxOptions,
  ComboboxOption as HeadlessComboboxOption,
  ComboboxInput as HeadlessComboboxInput,
  Label,
} from "@headlessui/react";
import React, { useRef, useState, useEffect } from "react";
import classNames from "classnames";
import { cn } from "@ui/cn";
import { useHoverDirty } from "react-use";
import { test } from "fuzzy";
import { Button } from "@ui/Button";
import { createPortal } from "react-dom";
import {
  useFloating,
  autoUpdate,
  offset as offsetMiddleware,
  flip,
  shift,
} from "@floating-ui/react";
import { usePortalContainer } from "./PortalContainer";
import { useIsNarrowScreen } from "./useIsNarrowScreen";

const MAX_DISPLAYED_OPTIONS = 100;

export type MultiSelectValue = string[] | "all";

export function MultiSelectCombobox({
  options,
  selectedOptions,
  setSelectedOptions,
  unit,
  unitPlural,
  label,
  labelHidden = false,
  Option,
  disableSearch = false,
  processFilterOption = (option) => option,
}: {
  options: string[];
  selectedOptions: MultiSelectValue;
  setSelectedOptions(newValue: MultiSelectValue): void;
  unit: string;
  unitPlural: string;
  label: string;
  labelHidden?: boolean;
  Option?: React.ComponentType<{ label: string; inButton: boolean }>;
  disableSearch?: boolean;
  processFilterOption?: (option: string) => string;
}) {
  const [query, setQuery] = useState("");
  const portalContainer = usePortalContainer();
  const [referenceElement, setReferenceElement] =
    useState<HTMLDivElement | null>(null);
  const [popperElement, setPopperElement] = useState<HTMLDivElement | null>(
    null,
  );

  // Force tabindex to 0 - HeadlessUI sets tabIndex={-1} on the button when
  // a ComboboxInput exists, but our input is inside the portal and only
  // rendered when open.
  useEffect(() => {
    if (referenceElement?.children[0]) {
      (referenceElement.children[0] as HTMLElement).tabIndex = 0;
    }
  }, [referenceElement]);

  const [isOpen, setIsOpen] = useState(false);
  const wasOpen = useRef(false);
  const isNarrow = useIsNarrowScreen();

  // Restore focus to the button when the dropdown closes
  useEffect(() => {
    if (wasOpen.current && !isOpen) {
      const button = referenceElement?.querySelector("button");
      button?.focus();
    }
    wasOpen.current = isOpen;
  }, [isOpen, referenceElement]);

  const { floatingStyles } = useFloating({
    placement: "bottom-start",
    middleware: [offsetMiddleware(4), flip(), shift()],
    whileElementsMounted: autoUpdate,
    elements: { reference: referenceElement, floating: popperElement },
  });

  // Get the width for the dropdown
  const getOptionsWidth = () => {
    if (!referenceElement) return undefined;
    return `${referenceElement.offsetWidth}px`;
  };

  const filteredOptions =
    query === ""
      ? options
      : options.filter((option) => test(query, processFilterOption(option)));

  const hasMoreThanMax = filteredOptions.length > MAX_DISPLAYED_OPTIONS;
  const displayedOptions = hasMoreThanMax
    ? filteredOptions.slice(0, MAX_DISPLAYED_OPTIONS)
    : filteredOptions;

  // Convert to internal array representation for Combobox
  const selectedArray = selectedOptions === "all" ? options : selectedOptions;

  const count =
    selectedOptions === "all"
      ? options.length
      : selectedOptions.filter((name) => name !== "_other").length;

  const displayValue =
    selectedOptions === "all"
      ? `All ${unitPlural}`
      : `${count} ${count !== 1 ? unitPlural : unit}`;

  const handleSelectAll = () => {
    if (selectedOptions === "all") {
      setSelectedOptions([]);
    } else {
      setSelectedOptions("all");
    }
  };

  return (
    <HeadlessCombobox
      value={selectedArray}
      onChange={(newSelection) => {
        // Check if all options are selected and convert to "all" state
        if (newSelection.length === options.length) {
          setSelectedOptions("all");
        } else {
          setSelectedOptions(newSelection);
        }
      }}
      multiple
    >
      {({ open }) => {
        // Update isOpen state when open changes
        if (open !== isOpen) {
          setIsOpen(open);
        }

        return (
          <>
            <Label
              className={classNames(
                "flex gap-1 text-sm font-semibold",
                labelHidden ? "hidden" : "mb-2",
              )}
              hidden={labelHidden}
            >
              {label}
            </Label>

            <div className="relative">
              <div
                ref={setReferenceElement}
                className={cn("relative flex items-center")}
              >
                <HeadlessComboboxButton
                  className={classNames(
                    "flex gap-2 w-full justify-between",
                    "truncate relative rounded-md py-1.5 px-1.5 text-left text-sm text-content-primary disabled:opacity-50 disabled:cursor-not-allowed disabled:hover:bg-background-secondary",
                    "border",
                    "focus:border-border-selected focus:outline-hidden bg-background-secondary hover:bg-background-tertiary",
                    open && "border-border-selected",
                  )}
                >
                  {displayValue}
                  <ChevronDownIcon
                    className={cn(
                      "relative z-30 -ml-6 size-5 text-content-primary transition-all",
                      open && "rotate-180",
                    )}
                  />
                </HeadlessComboboxButton>
              </div>

              {open &&
                createPortal(
                  <div
                    ref={isNarrow ? undefined : setPopperElement}
                    style={
                      isNarrow
                        ? undefined
                        : { ...floatingStyles, width: getOptionsWidth() }
                    }
                    className={
                      isNarrow ? "fixed inset-0 z-50 flex items-end" : "z-50"
                    }
                  >
                    {isNarrow && (
                      <div
                        className="absolute inset-0 bg-black/50"
                        aria-hidden="true"
                      />
                    )}
                    <HeadlessComboboxOptions
                      modal={false}
                      static
                      className={cn(
                        "scrollbar overflow-auto border bg-background-secondary focus:outline-hidden",
                        isNarrow
                          ? "relative max-h-[70dvh] w-full rounded-t-xl pb-2 text-sm shadow-xl"
                          : "max-h-60 w-fit max-w-80 min-w-full rounded-md pb-1 text-xs shadow-sm",
                      )}
                    >
                      <div className="min-w-fit">
                        {disableSearch ? (
                          // Hidden input to enable keyboard navigation
                          // (arrow keys, Enter, Escape) via HeadlessUI
                          <HeadlessComboboxInput
                            onChange={() => {}}
                            value=""
                            autoFocus
                            className="sr-only"
                            aria-hidden
                          />
                        ) : (
                          <div className="sticky top-0 left-0 z-20 flex w-full items-center gap-1 border-b bg-background-secondary px-2 pt-1">
                            <MagnifyingGlassIcon className="size-4 text-content-secondary" />
                            <HeadlessComboboxInput
                              onChange={(event) => setQuery(event.target.value)}
                              value={query}
                              autoFocus
                              placeholder={`Search ${unitPlural}...`}
                              className={classNames(
                                "placeholder:text-content-tertiary relative w-full py-1.5 text-left text-xs text-content-primary disabled:bg-background-tertiary disabled:text-content-secondary disabled:cursor-not-allowed",
                                "focus:outline-hidden bg-background-secondary",
                              )}
                            />
                          </div>
                        )}
                        {/* eslint-disable-next-line react/forbid-elements */}
                        <button
                          type="button"
                          className="w-full cursor-pointer p-2 pl-7 text-left text-content-primary hover:bg-background-tertiary"
                          onClick={handleSelectAll}
                        >
                          {selectedOptions === "all"
                            ? "Deselect all"
                            : "Select all"}
                        </button>

                        {displayedOptions.map((option) => (
                          <ComboboxOption
                            key={option}
                            value={option}
                            label={
                              Option ? (
                                <Option label={option} inButton={false} />
                              ) : (
                                option
                              )
                            }
                            onOnly={() => {
                              setSelectedOptions([option]);
                            }}
                          />
                        ))}

                        {hasMoreThanMax && (
                          <div className="w-fit min-w-full cursor-default px-2 py-1.5 text-content-tertiary select-none">
                            Too many items to display, use the search bar to
                            refine this list.
                          </div>
                        )}
                      </div>
                    </HeadlessComboboxOptions>
                  </div>,
                  portalContainer ?? document.body,
                )}
            </div>
          </>
        );
      }}
    </HeadlessCombobox>
  );
}

function ComboboxOption({
  value,
  label,
  onOnly,
}: {
  value: string;
  label: React.ReactNode | string;
  onOnly: () => void;
}) {
  const onlyRefs = useRef<HTMLElement | null>(null);
  // react-use's types predate React 19's nullable RefObject.
  const isHoveringOnly = useHoverDirty(onlyRefs as React.RefObject<Element>);
  return (
    <HeadlessComboboxOption
      value={value}
      className={({ focus }) =>
        classNames(
          "w-fit min-w-full flex gap-1 cursor-pointer select-none p-2 text-content-primary group",
          focus && "bg-background-tertiary",
        )
      }
      disabled={isHoveringOnly}
    >
      {({ selected }) => (
        <>
          {selected ? (
            <CheckIcon
              className="h-4 min-w-4 text-neutral-7 dark:text-neutral-4"
              aria-hidden="true"
            />
          ) : (
            <span className="min-w-4" />
          )}
          <span
            className={classNames(
              "flex gap-2 w-full whitespace-nowrap",
              selected && "font-semibold",
            )}
          >
            {label}
            <Button
              ref={onlyRefs}
              className="invisible text-xs font-normal text-content-secondary group-hover:visible hover:underline"
              variant="unstyled"
              onClick={onOnly}
            >
              only
            </Button>
          </span>
        </>
      )}
    </HeadlessComboboxOption>
  );
}
