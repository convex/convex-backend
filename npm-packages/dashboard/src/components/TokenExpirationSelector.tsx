import { Combobox } from "@ui/Combobox";
import { Tooltip } from "@ui/Tooltip";
import { InfoCircledIcon } from "@radix-ui/react-icons";

// Milliseconds of lifetime, or `null` for no expiration.
export type TokenExpirationValue = number | null;

const HOUR_MS = 60 * 60 * 1000;
const DAY_MS = 24 * HOUR_MS;

const PRESET_OPTIONS: { label: string; value: TokenExpirationValue }[] = [
  { label: "No expiration", value: null },
  { label: "1 hour", value: HOUR_MS },
  { label: "24 hours", value: 24 * HOUR_MS },
  { label: "7 days", value: 7 * DAY_MS },
  { label: "30 days", value: 30 * DAY_MS },
  { label: "90 days", value: 90 * DAY_MS },
  { label: "1 year", value: 365 * DAY_MS },
];

export function resolveExpirationTime(
  value: TokenExpirationValue,
): number | null {
  return value === null ? null : Date.now() + value;
}

function optionKey(value: TokenExpirationValue): string {
  return value === null ? "none" : String(value);
}

export function TokenExpirationSelector({
  value,
  onChange,
  className,
}: {
  value: TokenExpirationValue;
  onChange: (next: TokenExpirationValue) => void;
  className?: string;
}) {
  const selectedKey = optionKey(value);
  const comboboxOptions = PRESET_OPTIONS.map((o) => ({
    label: o.label,
    value: optionKey(o.value),
  }));

  return (
    <div className="flex flex-col gap-1">
      <Combobox
        label={
          <span className="flex items-center gap-1">
            Expiration
            <Tooltip tip="If set, the token will automatically be disabled after the expiration time.">
              <InfoCircledIcon className="text-content-tertiary" />
            </Tooltip>
          </span>
        }
        labelHidden={false}
        options={comboboxOptions}
        selectedOption={selectedKey}
        setSelectedOption={(key) => {
          if (key === null) return;
          const preset = PRESET_OPTIONS.find((o) => optionKey(o.value) === key);
          if (preset) onChange(preset.value);
        }}
        disableSearch
        className={className}
        buttonClasses="w-full"
        innerButtonClasses="w-full"
      />
    </div>
  );
}
