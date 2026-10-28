import {
  Disclosure,
  DisclosureButton,
  DisclosurePanel,
} from "@headlessui/react";
import { ChevronDownIcon, ChevronRightIcon } from "@radix-ui/react-icons";
import { useState } from "react";
import useSWR from "swr";
import { z } from "zod";
import { SyncSelection } from "system-udfs/convex/_system/frontend/common";
import { Callout } from "@ui/Callout";
import { Button } from "@ui/Button";
import { Checkbox } from "@ui/Checkbox";
import { TextInput } from "@ui/TextInput";
import {
  deploymentAuthMiddleware,
  useDeploymentIsDisconnected,
} from "@common/lib/deploymentApi";
import { deploymentFetch } from "@common/lib/fetching";
import {
  ExportTables,
  includeNewTables,
  selectTables,
  tableIncluded,
} from "./s3ExportSelection";

const schema = z.record(z.record(z.unknown()));

export function S3ExportTableSelector(props: {
  selection: SyncSelection;
  onChange: (selection: SyncSelection) => void;
  disabled: boolean;
}) {
  const disconnected = useDeploymentIsDisconnected();
  const { data, error, mutate } = useSWR(
    disconnected ? null : "/api/json_schemas?byComponent=true",
    async (...args: Parameters<typeof deploymentFetch>) => {
      const response = await deploymentFetch(...args);
      if (!response) throw new Error("Could not load tables");
      const components = schema.parse(response);
      return Object.fromEntries(
        Object.entries(components).map(([component, tables]) => [
          component,
          Object.keys(tables)
            .filter((name) => !name.startsWith("_"))
            .sort(),
        ]),
      );
    },
    { use: [deploymentAuthMiddleware] },
  );
  if (error || disconnected)
    return (
      <Callout variant="error" className="mt-0 max-w-none">
        <div className="flex w-full flex-wrap items-center justify-between gap-3">
          <div className="flex flex-col gap-1">
            <p className="text-sm font-medium">Could not load tables.</p>
            <p className="text-xs text-content-secondary">
              Try again to choose individual tables.
            </p>
          </div>
          <Button variant="neutral" size="sm" onClick={() => void mutate()}>
            Retry
          </Button>
        </div>
      </Callout>
    );
  if (!data)
    return (
      <p role="status" className="text-sm text-content-secondary">
        Loading tables…
      </p>
    );
  return <TableChoices {...props} tables={data} />;
}

export function TableChoices({
  selection,
  onChange,
  tables,
  disabled,
}: {
  selection: SyncSelection;
  onChange: (selection: SyncSelection) => void;
  tables: ExportTables;
  disabled: boolean;
}) {
  const [search, setSearch] = useState("");
  const entries = Object.entries(tables).sort(([a], [b]) => a.localeCompare(b));
  const visible: ExportTables = Object.fromEntries(
    entries
      .map(
        ([component, names]) =>
          [
            component,
            names.filter((name) =>
              `${component || "App"}/${name}`
                .toLowerCase()
                .includes(search.toLowerCase()),
            ),
          ] as const,
      )
      .filter(([, names]) => names.length > 0),
  );
  const total = entries.reduce((sum, [, names]) => sum + names.length, 0);
  const selected = entries.reduce(
    (sum, [component, names]) =>
      sum +
      names.filter((name) => tableIncluded(selection, component, name)).length,
    0,
  );
  const defaults = [
    selection._other !== "excluded",
    ...entries.map(([component]) => {
      const value = selection[component];
      return typeof value === "object"
        ? value._other !== "excluded"
        : value !== "excluded" && selection._other !== "excluded";
    }),
  ];
  const newTables = defaults.every(Boolean)
    ? true
    : defaults.some(Boolean)
      ? "indeterminate"
      : false;
  return (
    <div className="flex flex-col gap-3">
      <TextInput
        id="exportTableSearch"
        label="Search tables"
        placeholder="Table or component name"
        value={search}
        onChange={(event) => setSearch(event.target.value)}
      />
      <div className="flex flex-wrap items-center justify-between gap-2 text-xs">
        <span aria-live="polite">
          Exporting {selected.toLocaleString()} of {total.toLocaleString()}{" "}
          tables
        </span>
        <div className="flex gap-2">
          <Button
            variant="neutral"
            size="xs"
            disabled={disabled || !Object.keys(visible).length}
            onClick={() => onChange(selectTables(selection, visible, true))}
          >
            Select {search ? "results" : "all"}
          </Button>
          <Button
            variant="neutral"
            size="xs"
            disabled={disabled || !Object.keys(visible).length}
            onClick={() => onChange(selectTables(selection, visible, false))}
          >
            Deselect {search ? "results" : "all"}
          </Button>
        </div>
      </div>
      <div className="scrollbar max-h-72 overflow-y-auto rounded-sm border">
        {Object.entries(visible).map(([component, names]) => {
          const count = names.filter((name) =>
            tableIncluded(selection, component, name),
          ).length;
          return (
            <Disclosure
              key={`${component}:${!!search}`}
              as="div"
              defaultOpen={!!search || component === ""}
              className="border-b last:border-b-0"
            >
              <DisclosureButton className="flex w-full items-center gap-2 bg-background-tertiary px-3 py-2 text-left text-sm">
                {({ open }) => (
                  <>
                    {open ? (
                      <ChevronDownIcon className="shrink-0" />
                    ) : (
                      <ChevronRightIcon className="shrink-0" />
                    )}
                    {component || "App"}{" "}
                    <span className="text-content-secondary">
                      ({count}/{names.length})
                    </span>
                  </>
                )}
              </DisclosureButton>
              <DisclosurePanel className="flex flex-col gap-2 p-3">
                <label className="flex items-center gap-2 text-xs font-medium">
                  <Checkbox
                    aria-label={`Select all tables in ${component || "App"}`}
                    checked={
                      count === names.length
                        ? true
                        : count
                          ? "indeterminate"
                          : false
                    }
                    disabled={disabled}
                    onChange={() =>
                      onChange(
                        selectTables(
                          selection,
                          { [component]: names },
                          count !== names.length,
                        ),
                      )
                    }
                  />
                  All {search ? "matching " : ""}tables
                </label>
                {names.map((name) => (
                  <label
                    key={name}
                    className="flex min-w-0 items-center gap-2 text-sm"
                  >
                    <Checkbox
                      className="shrink-0"
                      aria-label={`${component || "App"}/${name}`}
                      checked={tableIncluded(selection, component, name)}
                      disabled={disabled}
                      onChange={() =>
                        onChange(
                          selectTables(
                            selection,
                            { [component]: [name] },
                            !tableIncluded(selection, component, name),
                          ),
                        )
                      }
                    />
                    <span className="break-all">{name}</span>
                  </label>
                ))}
              </DisclosurePanel>
            </Disclosure>
          );
        })}
        {!Object.keys(visible).length && (
          <p className="p-3 text-sm text-content-secondary">
            {total ? "No matching tables." : "No tables yet."}
          </p>
        )}
      </div>
      <label className="flex items-center gap-2 text-sm">
        <Checkbox
          aria-label="Automatically include new tables"
          checked={newTables}
          disabled={disabled}
          onChange={() =>
            onChange(includeNewTables(selection, tables, newTables !== true))
          }
        />
        Automatically include new tables
      </label>
      <p className="text-xs text-content-secondary">
        Applies to new tables across your app and components. Existing table
        choices stay the same.
      </p>
    </div>
  );
}
