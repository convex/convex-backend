import { useState } from "react";
import { ExclamationTriangleIcon } from "@radix-ui/react-icons";
import { Modal } from "@ui/Modal";
import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { cn } from "@ui/cn";
import {
  deploymentTypeLabel,
  deploymentTypeColorClasses,
} from "@common/lib/deploymentTypeColorClasses";
import { DeploymentType } from "generatedApi";

export type RolloutEntry = {
  name: string;
  operation: "set" | "delete";
  value: string | null;
  deploymentTypes: DeploymentType[];
};

export function DefaultEnvironmentVariablesRolloutDialog({
  rollout,
  onClose,
  onConfirm,
}: {
  rollout: RolloutEntry[];
  onClose: () => void;
  onConfirm: (selected: RolloutEntry[]) => void;
}) {
  const [selected, setSelected] = useState<Set<RolloutEntry>>(new Set());
  const allSelected = rollout.length > 0 && selected.size === rollout.length;

  const toggleEntry = (entry: RolloutEntry) => {
    setSelected((current) => {
      const next = new Set(current);
      if (next.has(entry)) next.delete(entry);
      else next.add(entry);
      return next;
    });
  };

  return (
    <Modal
      title="Review default environment variable changes"
      size="lg"
      onClose={onClose}
    >
      <div className="flex flex-col gap-4 text-sm">
        <RolloutTable
          rollout={rollout}
          selected={selected}
          allSelected={allSelected}
          onToggleEntry={toggleEntry}
          onToggleAll={() =>
            setSelected(allSelected ? new Set() : new Set(rollout))
          }
        />
        <RolloutFooter
          hasSelection={selected.size > 0}
          onClose={onClose}
          onConfirm={() =>
            onConfirm(rollout.filter((entry) => selected.has(entry)))
          }
        />
      </div>
    </Modal>
  );
}

function RolloutTable({
  rollout,
  selected,
  allSelected,
  onToggleEntry,
  onToggleAll,
}: {
  rollout: RolloutEntry[];
  selected: ReadonlySet<RolloutEntry>;
  allSelected: boolean;
  onToggleEntry: (entry: RolloutEntry) => void;
  onToggleAll: () => void;
}) {
  return (
    <div className="overflow-x-auto rounded-lg border">
      <table className="w-full min-w-240 table-fixed text-left">
        <colgroup>
          <col className="w-[20%]" />
          <col className="w-[24%]" />
          <col className="w-[28%]" />
          <col className="w-[28%]" />
        </colgroup>
        <thead className="border-b bg-background-tertiary text-xs text-content-secondary">
          <tr>
            <th scope="col" className="px-3 py-2">
              Environment variable name
            </th>
            <th scope="col" className="px-3 py-2">
              New value
            </th>
            <th scope="col" className="px-3 py-2">
              Affected deployment types
            </th>
            <th scope="col" className="px-3 py-2">
              <label className="flex cursor-pointer items-center gap-2">
                <Checkbox
                  className="shrink-0"
                  aria-label="Apply to existing deployments"
                  checked={
                    allSelected
                      ? true
                      : selected.size > 0
                        ? "indeterminate"
                        : false
                  }
                  onChange={onToggleAll}
                />
                Apply to existing deployments
              </label>
            </th>
          </tr>
        </thead>
        <tbody className="divide-y">
          {rollout.map((entry) => (
            <RolloutTableEntry
              key={JSON.stringify([
                entry.name,
                entry.operation,
                entry.deploymentTypes,
              ])}
              entry={entry}
              selected={selected.has(entry)}
              onToggle={() => onToggleEntry(entry)}
            />
          ))}
        </tbody>
      </table>
    </div>
  );
}

function RolloutTableEntry({
  entry,
  selected,
  onToggle,
}: {
  entry: RolloutEntry;
  selected: boolean;
  onToggle: () => void;
}) {
  const { name, operation, deploymentTypes, value } = entry;
  const label =
    operation === "delete"
      ? "Delete from existing deployments"
      : "Upsert to existing deployments";

  return (
    <tr>
      <td className="p-3">
        <code className="font-mono text-sm font-semibold break-all text-content-primary">
          {name}
        </code>
      </td>
      <td className="p-3">
        {value === null ? (
          <div className="w-full rounded-md border bg-background-primary px-2 py-1 font-mono text-sm text-content-secondary italic">
            Deleted
          </div>
        ) : (
          <pre className="max-h-10 w-full overflow-auto rounded-md border bg-background-primary px-2 py-1 font-mono text-sm whitespace-pre text-content-primary">
            {value}
          </pre>
        )}
      </td>
      <td className="p-3">
        <div className="flex flex-wrap gap-1">
          {deploymentTypes.map((deploymentType) => (
            <span
              key={deploymentType}
              className={cn(
                "rounded-sm px-2 py-0.5 text-xs font-medium",
                deploymentTypeColorClasses(deploymentType),
              )}
            >
              {deploymentTypeLabel(deploymentType)}
            </span>
          ))}
        </div>
      </td>
      <td className="p-3">
        <label className="flex cursor-pointer items-center gap-2">
          <Checkbox
            className="shrink-0"
            aria-label={`${label}: ${name}`}
            checked={selected}
            onChange={onToggle}
          />
          <span className="text-content-secondary">{label}</span>
        </label>
      </td>
    </tr>
  );
}

function RolloutFooter({
  hasSelection,
  onClose,
  onConfirm,
}: {
  hasSelection: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-4 pt-4">
      {hasSelection && (
        <p
          role="status"
          className="flex items-center gap-2 text-xs text-content-warning"
        >
          <ExclamationTriangleIcon
            className="size-3.5 shrink-0"
            aria-hidden="true"
          />
          Changes to deployments might not appear immediately. Any existing
          values will be overwritten.
        </p>
      )}
      <div className="ml-auto flex gap-2">
        <Button variant="neutral" onClick={onClose}>
          Cancel
        </Button>
        <Button onClick={onConfirm}>
          {hasSelection ? "Save and apply" : "Save changes"}
        </Button>
      </div>
    </div>
  );
}
