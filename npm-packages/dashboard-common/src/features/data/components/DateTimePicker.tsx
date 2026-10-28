import { cn } from "@ui/cn";
import { toDateTimeLocalValue } from "@common/lib/format";

type DateTimePickerProps = {
  date: Date;
  onChange: (date: Date) => void;
  onSave?: () => void;
  disabled?: boolean;
  autoFocus?: boolean;
  className?: string;
  "aria-label": string;
};

export function DateTimePicker({
  date,
  onChange,
  onSave,
  disabled = false,
  autoFocus = false,
  className,
  "aria-label": ariaLabel,
}: DateTimePickerProps) {
  return (
    <input
      aria-label={ariaLabel}
      autoFocus={autoFocus}
      type="datetime-local"
      className={cn(
        "focus:outline-none disabled:cursor-not-allowed disabled:bg-background-tertiary disabled:text-content-secondary",
        className,
      )}
      disabled={disabled}
      step={1}
      defaultValue={toDateTimeLocalValue(date, { includeSeconds: true })}
      onChange={(d) => {
        if (d.target.value) {
          onChange(new Date(d.target.value));
        }
      }}
      onKeyDown={(e) => {
        if (e.key === "Enter") {
          e.preventDefault();
          onSave?.();
        }
      }}
    />
  );
}
