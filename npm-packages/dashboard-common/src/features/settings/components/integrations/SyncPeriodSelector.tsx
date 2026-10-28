import { Combobox, Option } from "@ui/Combobox";
import { SyncPeriod } from "system-udfs/convex/_system/frontend/common";

const syncPeriodOptions: Option<SyncPeriod>[] = [
  { value: "daily", label: "Daily" },
  { value: "hourly", label: "Hourly" },
  { value: "continuous", label: "Continuous" },
];

const CHECK_INTERVAL: Record<SyncPeriod, string> = {
  daily: "every day",
  hourly: "every hour",
  continuous: "continuously",
};

export function SyncPeriodSelector({
  value,
  onChange,
}: {
  value: SyncPeriod;
  onChange: (period: SyncPeriod) => void;
}) {
  return (
    <div className="flex flex-col gap-1">
      <Combobox
        label="Check for changes"
        labelHidden={false}
        options={syncPeriodOptions}
        selectedOption={value}
        setSelectedOption={(period) => period && onChange(period)}
        allowCustomValue={false}
        disableSearch
        buttonClasses="w-full bg-inherit"
      />
      <div className="max-w-prose text-xs text-content-secondary">
        The first export starts when you save. After it catches up, Convex
        checks for changes {CHECK_INTERVAL[value]}.
      </div>
    </div>
  );
}
