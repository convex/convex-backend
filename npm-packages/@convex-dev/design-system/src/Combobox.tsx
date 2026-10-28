import { omit, isEqual } from "lodash-es";
import { useState, useEffect, useRef } from "react";
import {
  Combobox as HeadlessCombobox,
  ComboboxButton as HeadlessComboboxButton,
  ComboboxInput as HeadlessComboboxInput,
  ComboboxOption as HeadlessComboboxOption,
  ComboboxOptions as HeadlessComboboxOptions,
  Label,
} from "@headlessui/react";
import { ChevronDownIcon, MagnifyingGlassIcon } from "@radix-ui/react-icons";
import { cn } from "@ui/cn";
import fuzzy from "fuzzy";
import { Button, ButtonProps } from "@ui/Button";
import { createPortal } from "react-dom";
import {
  useFloating,
  autoUpdate,
  offset as offsetMiddleware,
  flip,
  shift,
} from "@floating-ui/react";
import { usePortalContainer } from "./PortalContainer";
import { Tooltip } from "./Tooltip";
import { Spinner } from "./Spinner";
import { useIsNarrowScreen } from "./useIsNarrowScreen";

const { test } = fuzzy;

// Raising this value above 100 may break pagination in the Convex dashboard.
export const MAX_DISPLAYED_OPTIONS = 100;

export type Option<T> = { label: string; value: T; disabled?: boolean };

export function Combobox<T>({
  options,
  optionsHeader,
  optionsWidth = "fixed",
  selectedOption,
  setSelectedOption,
  buttonClasses,
  innerButtonClasses,
  className,
  allowCustomValue = false,
  label,
  Option,
  searchPlaceholder = "Search...",
  disableSearch = false,
  buttonProps,
  disabled = false,
  unknownLabel = () => "Unknown option",
  labelHidden = true,
  processFilterOption = (option: string) => option,
  onFilterChange,
  placeholder = "Select an option",
  size = "md",
  icon,
  isLoadingOptions = false,
}: {
  label: React.ReactNode;
  labelHidden?: boolean;
  isLoadingOptions?: boolean;
  className?: string;
  optionsHeader?: React.ReactNode;
  onFilterChange?: (filter: string) => void;
  options: Readonly<Option<T>[]>;
  placeholder?: string;
  searchPlaceholder?: string;
  disableSearch?: boolean;
  // "full" only works if the options dropdown
  // fits inside of the ComboBox's ancestor elements,
  // or if the ancestors allow overflow.
  optionsWidth?: "full" | "fixed" | "fit";
  selectedOption?: T | null;
  setSelectedOption: (option: T | null) => void;
  buttonClasses?: string;
  buttonProps?: Omit<ButtonProps, "href">;
  innerButtonClasses?: string;
  allowCustomValue?: boolean;
  Option?: React.ComponentType<{
    label: string;
    value: T;
    inButton: boolean;
    disabled?: boolean;
  }>;
  disabled?: boolean;
  unknownLabel?: (value: T) => string;
  processFilterOption?: (option: string) => string;
  size?: "sm" | "md";
  icon?: React.ReactNode;
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
    if (
      wasOpen.current &&
      !isOpen &&
      document.activeElement === document.body
    ) {
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

  // Calculate width based on optionsWidth prop
  const getOptionsWidth = () => {
    if (!referenceElement) return undefined;

    if (optionsWidth === "full") {
      return `${referenceElement.offsetWidth}px`;
    }
    if (optionsWidth === "fixed") {
      return "240px";
    }
    return undefined; // auto width for "fit"
  };

  // If onFilterChange is defined, it indicates the creator of the Combobox will handle filtering options.
  const filtered =
    query === "" || onFilterChange !== undefined
      ? options
      : options.filter((option) =>
          test(query, processFilterOption(option.label)),
        );

  const hasMoreThanMaxOptions = filtered.length > MAX_DISPLAYED_OPTIONS;
  const displayedOptions = hasMoreThanMaxOptions
    ? filtered.slice(0, MAX_DISPLAYED_OPTIONS)
    : filtered;

  const selectedOptionData = options.find((o) =>
    isEqual(selectedOption, o.value),
  );

  // Reset the filter when the dropdown opens
  useEffect(() => {
    if (isOpen) {
      void onFilterChange?.("");
    }
  }, [isOpen, onFilterChange]);

  return (
    <HeadlessCombobox
      value={
        selectedOptionData ? selectedOptionData.value : (selectedOption ?? null)
      }
      onChange={(option) => {
        setSelectedOption(option);
        setQuery("");
      }}
      disabled={disabled}
    >
      {({ open }) => {
        // Update isOpen state when open changes
        // This effect runs on every render, but we only need to update
        // isOpen when open changes, so it's safe to call here
        if (open !== isOpen) {
          setIsOpen(open);
        }

        return (
          <>
            <Label
              hidden={labelHidden}
              className="text-left text-sm text-content-primary"
            >
              {label}
            </Label>
            <div className={cn("relative", className)}>
              <div
                ref={setReferenceElement}
                className={cn("relative flex w-60 items-center", buttonClasses)}
              >
                <Tooltip
                  tip={buttonProps?.tip}
                  side={buttonProps?.tipSide}
                  disableHoverableContent={
                    buttonProps?.tipDisableHoverableContent
                  }
                  asChild
                >
                  <HeadlessComboboxButton
                    as={Button}
                    variant="unstyled"
                    data-testid={`combobox-button-${label}`}
                    className={cn(
                      "group flex w-full items-center gap-1",
                      "relative truncate rounded-md text-left text-content-primary disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-background-secondary",
                      "border bg-background-secondary text-sm focus-visible:z-10 focus-visible:border-border-selected focus-visible:outline-hidden",
                      "hover:bg-background-tertiary",
                      "cursor-pointer",
                      open && "z-10 border-border-selected",
                      size === "sm" && "px-1.5 py-1 text-xs",
                      size === "md" && "p-1.5",
                      innerButtonClasses,
                    )}
                    {
                      // <HeadlessComboboxButton as={Button} tip="…" /> causes a state update loop since Headless UI 2.0
                      // (presumably because Headless UI and the tooltip both want to update the ref).
                      // To circumvent this issue, we place the <Tooltip /> component as a parent of <HeadlessComboboxButton />
                      ...omit(
                        buttonProps,
                        "tip",
                        "tipSide",
                        "tipDisableHoverableContent",
                      )
                    }
                  >
                    {icon}
                    <div className="truncate">
                      {selectedOptionData ? (
                        Option ? (
                          <Option
                            inButton
                            label={selectedOptionData.label}
                            value={selectedOptionData.value}
                            disabled={selectedOptionData.disabled}
                          />
                        ) : (
                          selectedOptionData.label
                        )
                      ) : selectedOption !== null &&
                        selectedOption !== undefined ? (
                        <span className="text-content-tertiary">
                          {unknownLabel(selectedOption)}
                        </span>
                      ) : (
                        <span className="text-content-tertiary">
                          {placeholder}
                        </span>
                      )}
                    </div>
                    {size === "md" && (
                      <ChevronDownIcon
                        className={cn(
                          "ml-auto size-4 text-content-primary transition-all",
                          open && "rotate-180",
                        )}
                      />
                    )}
                  </HeadlessComboboxButton>
                </Tooltip>
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
                        "scrollbar overflow-auto border bg-background-secondary",
                        isNarrow
                          ? "relative max-h-[70dvh] w-full rounded-t-xl pb-2 text-sm shadow-xl"
                          : "mt-1 max-h-59 rounded-md pb-1 text-xs shadow-sm",
                      )}
                      ref={(el) => {
                        el?.scrollTo?.(0, 0);
                      }}
                    >
                      {optionsHeader && (
                        <div className="border-b p-1 pb-2">{optionsHeader}</div>
                      )}
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
                          <div className="sticky top-0 z-10 flex w-full items-center gap-2 border-b bg-background-secondary px-3 pt-1">
                            {isLoadingOptions ? (
                              <div className="animate-fadeInFromLoading">
                                <Spinner className="size-3" />
                              </div>
                            ) : (
                              <MagnifyingGlassIcon className="animate-fadeInFromLoading text-content-secondary" />
                            )}
                            <HeadlessComboboxInput
                              onChange={(event) => {
                                setQuery(event.target.value);
                                onFilterChange?.(event.target.value);
                              }}
                              value={query}
                              autoFocus
                              className={cn(
                                "relative w-full truncate py-1.5 text-left text-xs text-content-primary placeholder:text-content-tertiary disabled:cursor-not-allowed disabled:bg-background-tertiary disabled:text-content-secondary",
                                "bg-background-secondary focus:outline-hidden",
                              )}
                              placeholder={searchPlaceholder}
                            />
                          </div>
                        )}
                        {displayedOptions.map((option, idx) => (
                          <HeadlessComboboxOption
                            key={idx}
                            value={option.value}
                            disabled={option.disabled}
                            className={({ focus }) =>
                              cn(
                                "relative w-fit min-w-full cursor-pointer px-3 py-1.5 text-content-primary select-none",
                                focus && "bg-background-tertiary",
                                option.disabled &&
                                  "cursor-not-allowed text-content-secondary opacity-75",
                              )
                            }
                          >
                            {({ selected }) => (
                              <span
                                className={cn(
                                  "block w-full whitespace-nowrap",
                                  selected && "font-semibold",
                                )}
                              >
                                {Option ? (
                                  <Option
                                    label={option.label}
                                    value={option.value}
                                    inButton={false}
                                    disabled={option.disabled}
                                  />
                                ) : (
                                  option.label
                                )}
                              </span>
                            )}
                          </HeadlessComboboxOption>
                        ))}

                        {hasMoreThanMaxOptions && (
                          <div className="relative w-fit min-w-full cursor-default px-3 py-1.5 text-content-tertiary select-none">
                            Too many options to display, use the search bar to
                            refine this list.
                          </div>
                        )}

                        {/* Allow users to type a custom value */}
                        {allowCustomValue &&
                          !isLoadingOptions &&
                          query.length > 0 &&
                          !filtered.some(
                            (x) => x.value === query || x.label === query,
                          ) && (
                            <HeadlessComboboxOption
                              value={query}
                              className={({ focus }) =>
                                `text-content-primary relative cursor-pointer w-60 select-none py-1 px-3 text-xs ${
                                  focus ? "bg-background-tertiary" : ""
                                }`
                              }
                            >
                              Unknown option: "{query}"
                            </HeadlessComboboxOption>
                          )}

                        {filtered.length === 0 && !allowCustomValue && (
                          <div className="overflow-hidden py-1 pl-4 text-ellipsis text-content-primary">
                            No options matching "{query}".
                          </div>
                        )}
                        {filtered.length === 0 && isLoadingOptions && (
                          <div className="m-4 overflow-hidden text-content-primary">
                            Loading options...
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
