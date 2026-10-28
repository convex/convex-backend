import { SyncSelection } from "system-udfs/convex/_system/frontend/common";

export function componentTablesIncluded(
  selection: SyncSelection,
): boolean | "indeterminate" {
  const included = selection._other !== "excluded";
  const overrides = Object.entries(selection).filter(
    ([component]) => component !== "" && component !== "_other",
  );
  const matchesDefault = overrides.every(([, value]) =>
    included
      ? typeof value === "object" &&
        value._other === "included" &&
        Object.keys(value).length === 1
      : value === "excluded",
  );
  return matchesDefault ? included : "indeterminate";
}

export function selectionWithComponents(
  selection: SyncSelection,
  included: boolean,
): SyncSelection {
  return {
    _other: included ? "included" : "excluded",
    // The empty component path is the app's own tables. Preserve their filters.
    "": selection[""] ?? {
      _other: selection._other === "excluded" ? "excluded" : "included",
    },
  };
}

export type ExportTables = Record<string, string[]>;

export function tableIncluded(
  selection: SyncSelection,
  component: string,
  table: string,
): boolean {
  const tables = selection[component];
  if (tables === "excluded") return false;
  if (typeof tables !== "object") return selection._other !== "excluded";
  return Object.hasOwn(tables, table)
    ? tables[table] !== "excluded"
    : tables._other !== "excluded";
}

export function selectTables(
  selection: SyncSelection,
  tables: ExportTables,
  included: boolean,
): SyncSelection {
  const next = { ...selection };
  for (const [component, names] of Object.entries(tables)) {
    const previous = selection[component];
    const defaults: Exclude<SyncSelection[string], string> =
      typeof previous === "object"
        ? previous
        : {
            _other:
              previous === "excluded"
                ? "excluded"
                : selection._other === "excluded"
                  ? "excluded"
                  : "included",
          };
    next[component] = { ...defaults };
    for (const name of names) {
      // Preserve an existing column filter when a selected table stays selected.
      next[component][name] = included
        ? typeof defaults[name] === "object"
          ? defaults[name]
          : { _other: "included" }
        : "excluded";
    }
  }
  return next;
}

export function includeNewTables(
  selection: SyncSelection,
  tables: ExportTables,
  included: boolean,
): SyncSelection {
  const next: SyncSelection = {
    ...selection,
    _other: included ? "included" : "excluded",
  };
  for (const component of new Set([
    ...Object.keys(tables),
    ...Object.keys(selection).filter((key) => key !== "_other"),
  ])) {
    const previous = selection[component];
    next[component] = {
      ...(typeof previous === "object" ? previous : {}),
      _other: included ? "included" : "excluded",
    };
    for (const table of tables[component] ?? []) {
      if (typeof previous !== "object" || !Object.hasOwn(previous, table)) {
        next[component][table] = tableIncluded(selection, component, table)
          ? { _other: "included" }
          : "excluded";
      }
    }
  }
  return next;
}
