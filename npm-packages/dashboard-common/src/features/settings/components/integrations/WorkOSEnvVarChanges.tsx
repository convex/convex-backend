import { useState } from "react";
import { Button } from "@ui/Button";
import {
  EyeOpenIcon,
  EyeNoneIcon,
  ArrowRightIcon,
} from "@radix-ui/react-icons";

export type EnvVarChange = {
  name: string;
  currentValue: string | null;
  newValue: string;
};

export function EnvVarChangeRow({ change }: { change: EnvVarChange }) {
  const [showValues, setShowValues] = useState(false);
  const isNew = change.currentValue === null;
  const isSecret = change.name === "WORKOS_API_KEY";
  const shouldShowCurrentValue = !isSecret || showValues;
  const shouldShowNewValue = !isSecret || showValues;

  return (
    <div className="flex flex-col gap-1.5 rounded-sm border bg-background-secondary p-3 text-xs">
      <div className="flex items-center gap-2">
        <div className="font-mono font-semibold text-content-primary">
          {change.name}
        </div>
        {isSecret && (
          <Button
            type="button"
            onClick={() => setShowValues(!showValues)}
            variant="neutral"
            size="sm"
            inline
            icon={showValues ? <EyeNoneIcon /> : <EyeOpenIcon />}
            tip={showValues ? "Hide value" : "Show value"}
          />
        )}
      </div>

      <div className="flex items-center gap-2 font-mono text-xs">
        {isNew ? (
          <>
            <div className="flex-1 text-content-secondary">Not set</div>
            <ArrowRightIcon className="size-3 shrink-0 text-content-secondary" />
            <div className="flex-1 overflow-x-auto text-content-success">
              <div className="inline-block min-w-0 whitespace-nowrap">
                {shouldShowNewValue ? change.newValue : "•••••••••"}
              </div>
            </div>
          </>
        ) : (
          <>
            <div className="flex-1 overflow-x-auto text-content-secondary">
              <div className="inline-block min-w-0 whitespace-nowrap line-through">
                {shouldShowCurrentValue ? change.currentValue : "•••••••••"}
              </div>
            </div>
            <ArrowRightIcon className="size-3 shrink-0 text-content-secondary" />
            <div className="flex-1 overflow-x-auto text-content-success">
              <div className="inline-block min-w-0 whitespace-nowrap">
                {shouldShowNewValue ? change.newValue : "•••••••••"}
              </div>
            </div>
          </>
        )}
      </div>
    </div>
  );
}
