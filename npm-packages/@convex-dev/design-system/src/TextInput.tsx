import { MagnifyingGlassIcon } from "@radix-ui/react-icons";
import classNames from "classnames";
import React, { forwardRef, useRef, useState, useLayoutEffect } from "react";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";
import { Spinner } from "@ui/Spinner";

type InputProps = {
  label?: string;
  labelHidden?: boolean;
  outerClassname?: string;
  onChange?: React.ChangeEventHandler<HTMLInputElement>;
  SearchIcon?: React.FC<{ className: string | undefined }>;
  /** If true, replaces the search icon with a loading spinner */
  isSearchLoading?: boolean;
  /** A non-interactive element appearing to the left of the input. */
  leftAddon?: React.ReactNode;
  /** A non-interactive element appearing to the right of the input. */
  rightAddon?: React.ReactNode;
  /** An interactive element appearing to the right of the input. */
  Icon?: React.FC<{ className: string | undefined }>;
  iconTooltip?: string;
  /** The action on `Icon`. */
  action?: () => void;
  error?: React.ReactNode;
  description?: React.ReactNode;
  id: string;
  type?: "text" | "search" | "email" | "time" | "password" | "number";
  size?: "sm" | "md";
};

export const TextInput = forwardRef<
  HTMLInputElement,
  InputProps & Omit<React.HTMLProps<HTMLInputElement>, "onChange" | "size">
>(
  (
    {
      outerClassname,
      label,
      labelHidden = false,
      Icon,
      iconTooltip,
      SearchIcon,
      isSearchLoading = false,
      leftAddon,
      rightAddon,
      action = () => {},
      error,
      description,
      className,
      onChange,
      type = "text",
      id,
      size = "md",
      ...rest
    },
    ref,
  ) => {
    const leftAddonRef = useRef<HTMLDivElement>(null);
    const [leftPadding, setLeftPadding] = useState<number | null>(null);

    useLayoutEffect(() => {
      if (leftAddonRef.current && leftAddon !== undefined) {
        const addonWidth = leftAddonRef.current.offsetWidth;
        // For search inputs, the search icon takes 24px (left-1.5 = 6px + icon 16px + spacing),
        // so the leftAddon starts at left-6 = 24px.
        // For non-search inputs, the leftAddon starts at left-1.5 = 6px.
        const addonStart = type === "search" ? 24 : 6;
        setLeftPadding(addonStart + addonWidth + 4);
      } else if (type === "search") {
        setLeftPadding(24); // Default pl-6 (1.5rem = 24px)
      } else {
        setLeftPadding(null);
      }
    }, [leftAddon, type]);

    return (
      <div ref={ref} className="flex w-full flex-col gap-1">
        <label
          className="text-left text-sm text-content-primary"
          htmlFor={id}
          hidden={type === "search" || labelHidden}
        >
          {label || id}
        </label>
        <div
          className={classNames(
            "relative flex items-center justify-between",
            outerClassname,
          )}
        >
          {type === "search" && (
            <div className="pointer-events-none absolute inset-y-0 left-1.5 flex items-center gap-1">
              {isSearchLoading ? (
                <div className="animate-fadeInFromLoading">
                  <Spinner className="size-3" />
                </div>
              ) : SearchIcon ? (
                <SearchIcon className="animate-fadeInFromLoading text-content-secondary" />
              ) : (
                <MagnifyingGlassIcon className="animate-fadeInFromLoading text-content-secondary" />
              )}
            </div>
          )}
          {leftAddon !== undefined && (
            <div
              ref={leftAddonRef}
              className={cn(
                "pointer-events-none absolute inset-y-0 left-1.5 flex items-center",
                type === "search" && "ml-5",
              )}
            >
              {leftAddon}
            </div>
          )}
          <input
            onChange={onChange}
            type={type}
            spellCheck={false}
            id={id}
            name={id}
            style={
              leftPadding !== null
                ? { paddingLeft: `${leftPadding}px` }
                : undefined
            }
            className={cn(
              error && "focus:border-content-error",
              !error && "text-content-primary focus:border-border-selected",
              "block rounded-md bg-background-secondary",
              size === "sm" ? "px-1.5 py-1 text-xs" : "p-1.5 px-2 text-sm",
              "border placeholder-content-tertiary focus:outline-hidden disabled:bg-background-tertiary disabled:text-content-secondary",
              "shrink grow truncate disabled:cursor-not-allowed",
              "min-w-0",
              leftPadding === null && "pl-2",
              rightAddon !== undefined && "pr-6",
              Icon && "pr-10",
              className,
            )}
            aria-label={labelHidden ? label : undefined}
            {...rest}
          />
          {rightAddon !== undefined && (
            <div className="pointer-events-none absolute inset-y-0 right-3 flex items-center">
              {rightAddon}
            </div>
          )}
          {Icon && (
            <Button
              size="sm"
              onClick={action}
              className="float-right mr-1.5 -ml-9.5"
              variant={error ? "danger" : "neutral"}
              inline
              icon={<Icon className="size-3.5" />}
              tip={iconTooltip}
            />
          )}
        </div>
        {error && (
          <p
            className="flex max-w-full animate-fadeInFromLoading gap-1 text-xs wrap-break-word text-content-errorSecondary"
            role="alert"
          >
            {error}
          </p>
        )}
        {description && !error && (
          <p className="max-w-prose animate-fadeInFromLoading text-xs text-content-secondary">
            {description}
          </p>
        )}
      </div>
    );
  },
);

TextInput.displayName = "TextInput";
