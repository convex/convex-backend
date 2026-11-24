import { cn } from "@ui/cn";
import { ReactNode, useCallback, useEffect, useRef, useState } from "react";
import { flushSync } from "react-dom";
import { Tooltip } from "./Tooltip";

export type SegmentedControlOption<T extends string> = {
  label: ReactNode;
  value: T;
  disabled?: boolean;
  /**
   * Shown when hovering the option, whether or not it is disabled. Use it to
   * name an icon-only label or to explain why the option is disabled.
   */
  tip?: ReactNode;
};

export function SegmentedControl<T extends string>({
  options,
  value,
  onChange,
  className,
  size = "md",
}: {
  options: SegmentedControlOption<T>[];
  /** `null` selects no option, for a choice that hasn't been made yet. */
  value: T | null;
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
  // Whether the highlight is appearing because a first option was selected,
  // as opposed to moving between options or rendering already selected.
  const [entering, setEntering] = useState(false);
  const previousValue = useRef(value);

  const measureHighlight = useCallback(() => {
    const container = containerRef.current;
    const button = value === null ? undefined : buttonRefs.current.get(value);
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
    setEntering(previousValue.current === null && value !== null);
    previousValue.current = value;
    setHighlightStyle(measureHighlight());
  }, [measureHighlight, options, value]);

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

    // Skip over disabled options. With no option selected, the arrows enter
    // the group from the matching end.
    let nextIndex = currentIndex === -1 && step === -1 ? 0 : currentIndex;
    for (let i = 0; i < options.length; i++) {
      nextIndex = (nextIndex + step + options.length) % options.length;
      if (!options[nextIndex].disabled) {
        break;
      }
    }

    // The loop ends on a disabled option when every option is disabled.
    const nextOption = options[nextIndex];
    if (nextIndex !== currentIndex && !nextOption.disabled) {
      e.preventDefault();
      onChange(nextOption.value);
      buttonRefs.current.get(nextOption.value)?.focus();
    }
  };

  const firstEnabledValue = options.find((option) => !option.disabled)?.value;

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
            entering
              ? // The highlight has nowhere to slide from, so it fades and
                // grows into place. Reduced motion keeps only the fade.
                "motion-safe:animate-scaleIn motion-reduce:animate-[fadeIn_0.2s_ease-out]"
              : animate && "transition-all duration-200 ease-in-out",
          )}
          style={{
            left: highlightStyle.left,
            width: highlightStyle.width,
          }}
        />
      )}
      {options.map((option) => {
        // The tab stop is the selected option, or the first enabled option
        // while nothing is selected.
        const isTabStop =
          value === null
            ? option.value === firstEnabledValue
            : value === option.value;
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
            tabIndex={!option.disabled && isTabStop ? 0 : -1}
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
        return option.tip ? (
          <Tooltip key={option.value} tip={option.tip} asChild>
            {button}
          </Tooltip>
        ) : (
          button
        );
      })}
    </div>
  );
}
