import { ConfirmationDialog } from "@ui/ConfirmationDialog";
import { cn } from "@ui/cn";
import {
  deploymentTypeLabel,
  deploymentTypeColorClasses,
} from "@common/lib/deploymentTypeColorClasses";
import { DeploymentType } from "generatedApi";

export type RolloutEntry = {
  name: string;
  operation: "set" | "delete";
  categories: { deploymentType: DeploymentType; total: number }[];
};

export function DefaultEnvironmentVariablesRolloutDialog({
  rollout,
  onClose,
  onConfirm,
}: {
  rollout: RolloutEntry[];
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <ConfirmationDialog
      dialogTitle="Apply to existing deployments?"
      confirmText="Save and apply"
      variant="primary"
      onClose={onClose}
      onConfirm={async () => onConfirm()}
      dialogBody={
        <div className="flex flex-col gap-3 text-sm">
          <p>
            Save all changes and schedule background updates to set or delete
            variables on every existing cloud deployment of the associated
            types. Existing values will be overwritten, and deleted variables
            will be removed. Updates might take some time to appear.
          </p>
          <ul className="flex flex-col gap-3">
            {rollout.map(({ name, operation, categories }) => {
              const total = categories.reduce(
                (sum, { total }) => sum + total,
                0,
              );
              const deploymentCategories = categories.filter(
                ({ total }) => total > 0,
              );
              return (
                <li
                  key={JSON.stringify([name, operation])}
                  className="overflow-hidden rounded-lg border"
                >
                  <div
                    className={cn(
                      "flex flex-wrap items-center justify-between gap-2 bg-background-secondary px-3 py-2",
                      deploymentCategories.length > 0 && "border-b",
                    )}
                  >
                    <div className="flex min-w-0 flex-wrap items-center gap-2">
                      <code className="min-w-0 font-semibold break-all">
                        {name}
                      </code>
                      <span
                        className={cn(
                          "text-xs font-medium",
                          operation === "delete"
                            ? "text-content-errorSecondary"
                            : "text-content-secondary",
                        )}
                      >
                        {operation === "delete"
                          ? "Delete variable"
                          : "Set variable"}
                      </span>
                    </div>
                    <span className="text-xs text-content-secondary">
                      <span className="font-semibold text-content-primary tabular-nums">
                        {total.toLocaleString()}
                      </span>{" "}
                      {total === 1 ? "deployment" : "deployments"}
                    </span>
                  </div>
                  {deploymentCategories.length > 0 && (
                    <dl
                      aria-label={`${name} deployments by type`}
                      className="flex flex-col gap-2 p-3"
                    >
                      {deploymentCategories.map(({ deploymentType, total }) => (
                        <div
                          key={deploymentType}
                          className="flex items-center justify-between gap-3"
                        >
                          <dt
                            className={cn(
                              "rounded-sm px-2 py-0.5 text-xs font-medium",
                              deploymentTypeColorClasses(deploymentType),
                            )}
                          >
                            {deploymentTypeLabel(deploymentType)}
                          </dt>
                          <dd className="whitespace-nowrap text-content-secondary">
                            <span className="font-medium text-content-primary tabular-nums">
                              {total.toLocaleString()}
                            </span>{" "}
                            {total === 1 ? "deployment" : "deployments"}
                          </dd>
                        </div>
                      ))}
                    </dl>
                  )}
                </li>
              );
            })}
          </ul>
        </div>
      }
    />
  );
}
