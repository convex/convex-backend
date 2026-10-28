import { useEffect, useRef } from "react";
import { cn } from "./cn";

export function Checkbox({
  checked,
  "aria-label": ariaLabel = "Selected",
  className,
  onChange,
  onKeyDown,
  disabled = false,
  id = undefined,
}: {
  checked: boolean | "indeterminate";
  "aria-label"?: string;
  className?: string;
  onChange: React.EventHandler<React.SyntheticEvent<HTMLInputElement>>;
  onKeyDown?: (event: React.KeyboardEvent<HTMLInputElement>) => void;
  disabled?: boolean;
  id?: string;
}) {
  const inputRef = useRef<HTMLInputElement>(null);
  const isIndeterminate = checked === "indeterminate";
  useEffect(() => {
    if (inputRef.current !== null) {
      inputRef.current.indeterminate = isIndeterminate;
    }
  }, [isIndeterminate]);
  const checkedBool = checked === "indeterminate" ? false : checked;
  return (
    <input
      id={id}
      ref={inputRef}
      tabIndex={0}
      type="checkbox"
      className={cn(
        "form-checkbox size-3.5 rounded-sm enabled:cursor-pointer enabled:hover:outline enabled:hover:outline-content-primary disabled:cursor-not-allowed disabled:opacity-50",
        "focus:ring-0 focus:ring-offset-0 focus:outline-none focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-border-selected focus-visible:outline-solid",
        "bg-background-secondary text-util-accent ring-offset-background-secondary checked:bg-util-accent",
        // eslint-disable-next-line no-restricted-syntax -- using text-content-link the enabled hover color
        "enabled:hover:text-content-link",
        className,
      )}
      onChange={onChange}
      onKeyDown={(event) => {
        if (event.key === "Enter") {
          onChange(event);
          return;
        }
        onKeyDown?.(event);
      }}
      disabled={disabled ?? false}
      checked={checkedBool}
      aria-checked={checked === "indeterminate" ? "mixed" : checkedBool}
      aria-label={ariaLabel}
    />
  );
}
