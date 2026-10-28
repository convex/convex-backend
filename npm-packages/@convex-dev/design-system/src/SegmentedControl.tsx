import { cn } from "@ui/cn";
import { ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { Tooltip } from "./Tooltip";

export type SegmentedControlOption<T extends string> = {
  label: ReactNode;
  value: T;
  disabled?: boolean;
  /** Shown in a tooltip when the option is disabled. */
  disabledTooltip?: ReactNode;
};

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  className,
  size = "md",
}: {
  options: SegmentedControlOption<T>[];
  value: T;
  onChange: (value: T) => void;
  className?: string;
  size?: "sm" | "md";
}) {
  const containerRef = useRef<HTMLDivElement>(null);
  const buttonRefs = useRef<Map<T, HTMLButtonElement>>(new Map());
  const [highlightStyle, setHighlightStyle] = useState<{
    left: number;
    width: number;
  } | null>(null);
  const [animate, setAnimate] = useState(true);

  const measureHighlight = useCallback(() => {
    const container = containerRef.current;
    const button = buttonRefs.current.get(value);
    if (container && button) {
      const containerRect = container.getBoundingClientRect();
      const buttonRect = button.getBoundingClientRect();
      return {
        left: buttonRect.left - containerRect.left,
        width: buttonRect.width,
      };
    }
    return null;
  }, [value]);

  // Keep a ref to the latest measure function so the ResizeObserver
  // can call it without being re-created on every value change.
  const measureRef = useRef(measureHighlight);
  useEffect(() => {
    measureRef.current = measureHighlight;
  }, [measureHighlight]);

  // Animate when value or options change
  useEffect(() => {
    setAnimate(true);
    setHighlightStyle(measureHighlight());
  }, [measureHighlight, options]);

  // Instantly reposition on resize (no animation)
  // This is done to avoid having a frame where the
  // highlighted section overlaps text from a different option
  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;
    const observer = new ResizeObserver(() => {
      flushSync(() => {
        setAnimate(false);
        setHighlightStyle(measureRef.current());
      });
    });
    observer.observe(container);
    return () => observer.disconnect();
  }, []);

  const handleKeyDown = (e: React.KeyboardEvent) => {
    const currentIndex = options.findIndex((o) => o.value === value);
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (step === 0) {
      return;
    }

    // Skip over disabled options.
    let nextIndex = currentIndex;
    for (let i = 0; i < options.length; i++) {
      nextIndex = (nextIndex + step + options.length) % options.length;
      if (!options[nextIndex].disabled) {
        break;
      }
    }

    if (nextIndex !== currentIndex) {
      e.preventDefault();
      const nextOption = options[nextIndex];
      onChange(nextOption.value);
      buttonRefs.current.get(nextOption.value)?.focus();
    }
  };

  return (
    <div
      ref={containerRef}
      role="radiogroup"
      className={cn(
        "relative inline-flex rounded-full border bg-background-tertiary",
        size === "sm" ? "p-0.5" : "p-1",
        className,
      )}
    >
      {highlightStyle && (
        <div
          className={cn(
            "absolute rounded-full bg-background-secondary shadow-sm",
            size === "sm" ? "inset-y-0.5" : "inset-y-1",
            animate && "transition-all duration-200 ease-in-out",
          )}
          style={{
            left: highlightStyle.left,
            width: highlightStyle.width,
          }}
        />
      )}
      {options.map((option) => {
        const button = (
          // eslint-disable-next-line react/forbid-elements -- We need a native button here to implement custom radiogroup semantics and keyboard focus management.
          <button
            key={option.value}
            ref={(el) => {
              if (el) {
                buttonRefs.current.set(option.value, el);
              } else {
                buttonRefs.current.delete(option.value);
              }
            }}
            type="button"
            role="radio"
            aria-checked={value === option.value}
            // Use aria-disabled (not the native disabled attribute) so the
            // option still receives hover events for its tooltip.
            aria-disabled={option.disabled || undefined}
            tabIndex={!option.disabled && value === option.value ? 0 : -1}
            className={cn(
              "relative z-10 rounded-full font-medium transition-colors duration-200 outline-none focus-visible:ring-2 focus-visible:ring-border-selected focus-visible:ring-inset",
              size === "sm" ? "px-2.5 py-1 text-xs" : "px-4 py-1.5 text-sm",
              option.disabled
                ? "cursor-not-allowed text-content-secondary opacity-50"
                : "text-content-primary",
            )}
            onClick={() => !option.disabled && onChange(option.value)}
            onKeyDown={handleKeyDown}
          >
            {option.label}
          </button>
        );
        return option.disabled && option.disabledTooltip ? (
          <Tooltip key={option.value} tip={option.disabledTooltip} asChild>
            {button}
          </Tooltip>
        ) : (
          button
        );
      })}
    </div>
  );
}
