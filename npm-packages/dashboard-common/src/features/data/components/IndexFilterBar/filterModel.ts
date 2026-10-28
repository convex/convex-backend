import { GenericDocument } from "convex/server";
import { JSONValue, Value, convexToJson } from "convex/values";
import {
  DatabaseIndexFilter,
  DatabaseIndexFilterClause,
  Filter,
  FilterByBuiltin,
  FilterByIndexRange,
  FilterByType,
  FilterExpression,
  SearchIndexFilter,
  SearchIndexFilterClause,
  isValidFilter,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { UNDEFINED_PLACEHOLDER } from "system-udfs/convex/_system/frontend/lib/values";
import { Option } from "@ui/Combobox";
import { Index } from "@common/features/data/lib/api";
import { stringifyValue } from "@common/lib/stringifyValue";

// The query rules this model encodes:
//  - Indexed clauses must be a prefix of the index's fields, in order.
//  - Only the last indexed clause may be a range; nothing may follow it.
//  - Sorting always follows the selected index (asc/desc).
//  - Search indexes take one search string plus equality clauses on their
//    filter fields, always in ascending (relevance) order.
//  - Unindexed clauses ("table scans") apply on top of the index results.
// Everything else here is derived from those rules so the UI can offer only
// valid next moves instead of disabling invalid ones.

export type DatabaseIndexDef = {
  kind: "database";
  name: string;
  fields: string[];
  system: boolean;
};

export type SearchIndexDef = {
  kind: "search";
  name: string;
  searchField: string;
  filterFields: string[];
};

export type IndexDef = DatabaseIndexDef | SearchIndexDef;

export const CREATION_TIME_INDEX: DatabaseIndexDef = {
  kind: "database",
  name: "by_creation_time",
  fields: ["_creationTime"],
  system: true,
};

const ID_INDEX: DatabaseIndexDef = {
  kind: "database",
  name: "by_id",
  fields: ["_id"],
  system: true,
};

export const EMPTY_FILTERS: FilterExpression = { clauses: [] };

// Staged indexes can't be queried yet, and vector indexes have no
// pagination story on the Data page, so neither is offered.
export function buildIndexDefs(indexes: Index[] | undefined): IndexDef[] {
  const userDefs = (indexes ?? []).flatMap<IndexDef>((index) => {
    if (index.staged) return [];
    if (Array.isArray(index.fields)) {
      return [
        {
          kind: "database" as const,
          name: index.name,
          fields: index.fields,
          system: false,
        },
      ];
    }
    if ("searchField" in index.fields) {
      return [
        {
          kind: "search" as const,
          name: index.name,
          searchField: index.fields.searchField,
          filterFields: index.fields.filterFields,
        },
      ];
    }
    return [];
  });
  return [CREATION_TIME_INDEX, ID_INDEX, ...userDefs];
}

export function findIndexDef(
  defs: IndexDef[],
  name: string | undefined,
): IndexDef | undefined {
  return defs.find((d) => d.name === name);
}

export function databaseIndexDefs(defs: IndexDef[]): DatabaseIndexDef[] {
  return defs.filter((d): d is DatabaseIndexDef => d.kind === "database");
}

export function searchIndexDefs(defs: IndexDef[]): SearchIndexDef[] {
  return defs.filter((d): d is SearchIndexDef => d.kind === "search");
}

export function isSearchFilter(
  index: FilterExpression["index"],
): index is SearchIndexFilter {
  return index !== undefined && "search" in index;
}

export function isDatabaseFilter(
  index: FilterExpression["index"],
): index is DatabaseIndexFilter {
  return index !== undefined && !("search" in index);
}

export function isRangeClause(
  clause: DatabaseIndexFilterClause,
): clause is FilterByIndexRange {
  return clause.type === "indexRange";
}

// The applied prefix of indexed clauses. Older URLs pad the index with
// disabled trailing clauses (one per index field); those carry no meaning.
export function enabledIndexClauses(
  expr: FilterExpression,
): DatabaseIndexFilterClause[] {
  if (!isDatabaseFilter(expr.index)) return [];
  const result: DatabaseIndexFilterClause[] = [];
  for (const clause of expr.index.clauses) {
    if (!clause.enabled) break;
    result.push(clause);
  }
  return result;
}

export function currentDatabaseIndex(
  defs: IndexDef[],
  expr: FilterExpression,
): DatabaseIndexDef {
  if (isDatabaseFilter(expr.index)) {
    const def = findIndexDef(defs, expr.index.name);
    if (def?.kind === "database") return def;
  }
  return CREATION_TIME_INDEX;
}

// Indexes that can serve the currently applied indexed clauses without
// changing their meaning: same field at every used position.
function compatibleIndexes(
  defs: IndexDef[],
  expr: FilterExpression,
): DatabaseIndexDef[] {
  const used = enabledIndexClauses(expr);
  const current = currentDatabaseIndex(defs, expr);
  const usedFields = used.map((_, i) => current.fields[i]);
  return databaseIndexDefs(defs).filter(
    (d) =>
      d.fields.length >= usedFields.length &&
      usedFields.every((f, i) => d.fields[i] === f),
  );
}

export type NextIndexedField = { field: string; index: DatabaseIndexDef };

// Fields that can be added as the next indexed clause right now. Empty when
// the last clause is a range (nothing may follow it) or a search index is
// active (see `nextSearchFilterFields`).
export function nextIndexedFields(
  defs: IndexDef[],
  expr: FilterExpression,
): NextIndexedField[] {
  if (isSearchFilter(expr.index)) return [];
  const used = enabledIndexClauses(expr);
  if (used.length > 0 && isRangeClause(used[used.length - 1])) return [];
  const current = currentDatabaseIndex(defs, expr);
  const seen = new Set<string>();
  const result: NextIndexedField[] = [];
  // The current index goes first so its next field wins ties.
  const candidates = [
    current,
    ...compatibleIndexes(defs, expr).filter((d) => d.name !== current.name),
  ];
  for (const index of candidates) {
    const field = index.fields[used.length];
    if (field === undefined || seen.has(field)) continue;
    seen.add(field);
    result.push({ field, index });
  }
  return result;
}

export function nextSearchFilterFields(
  defs: IndexDef[],
  expr: FilterExpression,
): string[] {
  if (!isSearchFilter(expr.index)) return [];
  const def = findIndexDef(defs, expr.index.name);
  if (def?.kind !== "search") return [];
  const used = new Set(
    expr.index.clauses.filter((c) => c.enabled).map((c) => c.field),
  );
  return def.filterFields.filter((f) => !used.has(f));
}

// Among indexes that can take `field` at the next position, prefer the one
// already in use, then the shortest (its sort is closest to creation-time
// order).
function chooseIndex(
  candidates: DatabaseIndexDef[],
  expr: FilterExpression,
): DatabaseIndexDef {
  const currentName = isDatabaseFilter(expr.index)
    ? expr.index.name
    : undefined;
  return [...candidates].sort((a, b) => {
    if (a.name === currentName) return -1;
    if (b.name === currentName) return 1;
    return a.fields.length - b.fields.length;
  })[0];
}

// The applied indexed clauses stay as they are; only the index name changes.
function withDatabaseIndex(
  expr: FilterExpression,
  index: DatabaseIndexDef,
  clauses: DatabaseIndexFilterClause[],
): FilterExpression {
  return {
    ...expr,
    order: isSearchFilter(expr.index) ? undefined : expr.order,
    index: {
      name: index.name,
      clauses: clauses as DatabaseIndexFilter["clauses"],
    },
  };
}

export function addIndexedClause(
  defs: IndexDef[],
  expr: FilterExpression,
  field: string,
  clause: DatabaseIndexFilterClause,
): FilterExpression {
  const used = enabledIndexClauses(expr);
  const candidates = compatibleIndexes(defs, expr).filter(
    (d) => d.fields[used.length] === field,
  );
  if (candidates.length === 0) {
    throw new Error(`No index can filter on ${field} at this position`);
  }
  const index = chooseIndex(candidates, expr);
  return withDatabaseIndex(expr, index, [
    ...used,
    { ...clause, enabled: true },
  ]);
}

export function setIndexedClause(
  defs: IndexDef[],
  expr: FilterExpression,
  position: number,
  clause: DatabaseIndexFilterClause,
): FilterExpression {
  const used = enabledIndexClauses(expr);
  const next = [...used];
  next[position] = { ...clause, enabled: true };
  const trimmed = isRangeClause(clause) ? next.slice(0, position + 1) : next;
  return withDatabaseIndex(expr, currentDatabaseIndex(defs, expr), trimmed);
}

// Removing an indexed clause also removes every clause after it, since the
// prefix rule leaves them nothing to attach to.
export function removeIndexedClause(
  defs: IndexDef[],
  expr: FilterExpression,
  position: number,
): { expr: FilterExpression; removed: number } {
  const used = enabledIndexClauses(expr);
  return {
    expr: withDatabaseIndex(
      expr,
      currentDatabaseIndex(defs, expr),
      used.slice(0, position),
    ),
    removed: used.length - position,
  };
}

// Switching index explicitly keeps the applied clauses whose field matches
// at the same position and drops the rest.
export function setDatabaseIndex(
  defs: IndexDef[],
  expr: FilterExpression,
  index: DatabaseIndexDef,
): FilterExpression {
  const current = currentDatabaseIndex(defs, expr);
  const used = enabledIndexClauses(expr);
  const kept: DatabaseIndexFilterClause[] = [];
  for (const [i, clause] of used.entries()) {
    if (current.fields[i] !== index.fields[i]) break;
    kept.push(clause);
  }
  return withDatabaseIndex(expr, index, kept);
}

export function setSearchIndex(
  expr: FilterExpression,
  index: SearchIndexDef,
): FilterExpression {
  const previous = isSearchFilter(expr.index) ? expr.index.search : "";
  return {
    clauses: expr.clauses,
    order: "asc",
    index: { name: index.name, search: previous, clauses: [] },
  };
}

export function setSearchText(
  expr: FilterExpression,
  search: string,
): FilterExpression {
  if (!isSearchFilter(expr.index)) return expr;
  return { ...expr, index: { ...expr.index, search } };
}

// The search string and the index name stay as they are; only the clauses
// change. `withDatabaseIndex`'s counterpart, and like it the one place the
// expression is rebuilt, so every edit below is just a list of clauses.
function withSearchClauses(
  expr: FilterExpression,
  clauses: SearchIndexFilterClause[],
): FilterExpression {
  if (!isSearchFilter(expr.index)) return expr;
  return { ...expr, index: { ...expr.index, clauses } };
}

// The applied search clauses, in the order their chips appear.
function enabledSearchClauses(
  expr: FilterExpression,
): SearchIndexFilterClause[] {
  return isSearchFilter(expr.index)
    ? expr.index.clauses.filter((c) => c.enabled)
    : [];
}

export function addSearchFilterClause(
  expr: FilterExpression,
  field: string,
  value: JSONValue | undefined,
): FilterExpression {
  return withSearchClauses(expr, [
    ...enabledSearchClauses(expr),
    { field, enabled: true, value },
  ]);
}

export function setSearchFilterClause(
  expr: FilterExpression,
  position: number,
  value: JSONValue | undefined,
): FilterExpression {
  const clauses = enabledSearchClauses(expr);
  clauses[position] = { ...clauses[position], value };
  return withSearchClauses(expr, clauses);
}

export function removeSearchFilterClause(
  expr: FilterExpression,
  position: number,
): FilterExpression {
  return withSearchClauses(
    expr,
    enabledSearchClauses(expr).filter((_, i) => i !== position),
  );
}

// Leaving a search goes back to creation-time order with nothing applied,
// since the search string and its equality clauses have no database
// equivalent.
export function clearSearchIndex(expr: FilterExpression): FilterExpression {
  if (!isSearchFilter(expr.index)) return expr;
  return { clauses: [], index: undefined, order: undefined };
}

export type IndexForField = {
  index: DatabaseIndexDef;
  // Applied indexed clauses that survive switching to this index.
  kept: number;
  dropped: number;
};

// Database indexes that could take `field` as a clause after switching to
// them: the ones continuing the applied prefix, and the ones starting over
// from `field` at the cost of dropping the applied indexed clauses.
export function indexesForField(
  defs: IndexDef[],
  expr: FilterExpression,
  field: string,
): IndexForField[] {
  if (isSearchFilter(expr.index)) return [];
  const current = currentDatabaseIndex(defs, expr);
  const used = enabledIndexClauses(expr);
  return databaseIndexDefs(defs).flatMap((index) => {
    let kept = 0;
    while (kept < used.length && current.fields[kept] === index.fields[kept]) {
      kept += 1;
    }
    // An index range ends the query the backend can run ("Index range not
    // supported" otherwise), so nothing can be appended to a kept prefix that
    // ends in one. Dropping the range (kept === 0) is still on the table.
    if (kept > 0 && isRangeClause(used[kept - 1])) return [];
    if (index.fields[kept] !== field) return [];
    return [{ index, kept, dropped: used.length - kept }];
  });
}

// Every way to put a filter on `field` right now, best first. A search is
// never first: it throws the rest of the query away, so it stays an explicit
// choice (see `bestFieldOption`).
export type FieldOption =
  // An equality on a filter field of the search index already in use.
  | { kind: "searchFilter"; index: SearchIndexDef }
  | ({ kind: "index" } & IndexForField)
  // Starting a search on an index whose search field is this one.
  | { kind: "search"; index: SearchIndexDef; dropped: number }
  | { kind: "scan" };

export function optionsForField(
  defs: IndexDef[],
  expr: FilterExpression,
  field: string,
): FieldOption[] {
  const current = isSearchFilter(expr.index)
    ? findIndexDef(defs, expr.index.name)
    : undefined;
  const options: FieldOption[] = [];
  if (
    current?.kind === "search" &&
    nextSearchFilterFields(defs, expr).includes(field)
  ) {
    options.push({ kind: "searchFilter", index: current });
  }
  for (const forField of [...indexesForField(defs, expr, field)].sort(
    (a, b) => a.dropped - b.dropped,
  )) {
    options.push({ kind: "index", ...forField });
  }
  // A search keeps nothing but the search string itself.
  const droppedBySearch =
    enabledIndexClauses(expr).length +
    enabledSearchClauses(expr).length +
    expr.clauses.length;
  for (const index of searchIndexDefs(defs)) {
    if (index.searchField !== field || index.name === current?.name) continue;
    options.push({ kind: "search", index, dropped: droppedBySearch });
  }
  options.push({ kind: "scan" });
  return options;
}

export function bestFieldOption(options: FieldOption[]): FieldOption {
  return options.find((o) => o.kind !== "search") ?? { kind: "scan" };
}

export function switchIndexAndAddClause(
  defs: IndexDef[],
  expr: FilterExpression,
  index: DatabaseIndexDef,
  field: string,
  clause: DatabaseIndexFilterClause,
): FilterExpression {
  return addIndexedClause(
    defs,
    setDatabaseIndex(defs, expr, index),
    field,
    clause,
  );
}

export function addScanClause(
  expr: FilterExpression,
  clause: Filter,
): FilterExpression {
  return { ...expr, clauses: [...expr.clauses, clause] };
}

export function setScanClause(
  expr: FilterExpression,
  position: number,
  clause: Filter,
): FilterExpression {
  const clauses = [...expr.clauses];
  clauses[position] = { ...clause, enabled: true };
  return { ...expr, clauses };
}

export function removeScanClause(
  expr: FilterExpression,
  position: number,
): FilterExpression {
  return { ...expr, clauses: expr.clauses.filter((_, i) => i !== position) };
}

export function setOrder(
  expr: FilterExpression,
  order: "asc" | "desc",
): FilterExpression {
  return { ...expr, order };
}

export function currentOrder(expr: FilterExpression): "asc" | "desc" {
  return expr.order ?? "desc";
}

// Equality clauses pin their field to one value, so the visible order is
// decided by the first index field that isn't pinned. A trailing range
// clause still sorts by its own field.
export function effectiveSortField(
  defs: IndexDef[],
  expr: FilterExpression,
): string {
  if (isSearchFilter(expr.index)) {
    const def = findIndexDef(defs, expr.index.name);
    return def?.kind === "search" ? def.searchField : "_creationTime";
  }
  const index = currentDatabaseIndex(defs, expr);
  const used = enabledIndexClauses(expr);
  let pinned = 0;
  while (pinned < used.length && !isRangeClause(used[pinned])) pinned += 1;
  return index.fields[Math.min(pinned, index.fields.length - 1)];
}

export type SortOption =
  | { kind: "toggle" }
  | { kind: "switch"; index: DatabaseIndexDef; dropsClauses: boolean }
  | { kind: "unavailable" };

// How a click on `field`'s column header would sort. Prefers an index that
// keeps the applied clauses (same prefix, `field` next), then any index
// starting with `field` at the cost of dropping the applied indexed clauses.
export function sortOptionForField(
  defs: IndexDef[],
  expr: FilterExpression,
  field: string,
): SortOption {
  if (isSearchFilter(expr.index)) return { kind: "unavailable" };
  if (effectiveSortField(defs, expr) === field) return { kind: "toggle" };
  const used = enabledIndexClauses(expr);
  const pinned = used.filter((c) => !isRangeClause(c)).length;
  if (pinned === used.length) {
    const compatible = compatibleIndexes(defs, expr).filter(
      (d) => d.fields[pinned] === field,
    );
    if (compatible.length > 0) {
      return {
        kind: "switch",
        index: chooseIndex(compatible, expr),
        dropsClauses: false,
      };
    }
  }
  const fresh = databaseIndexDefs(defs).filter((d) => d.fields[0] === field);
  if (fresh.length > 0) {
    return {
      kind: "switch",
      index: chooseIndex(fresh, { ...expr, index: undefined }),
      dropsClauses: used.length > 0,
    };
  }
  return { kind: "unavailable" };
}

export function sortByField(
  defs: IndexDef[],
  expr: FilterExpression,
  field: string,
): FilterExpression | undefined {
  const option = sortOptionForField(defs, expr, field);
  switch (option.kind) {
    case "toggle":
      return setOrder(expr, currentOrder(expr) === "asc" ? "desc" : "asc");
    case "switch": {
      const switched = option.dropsClauses
        ? withDatabaseIndex(expr, option.index, [])
        : setDatabaseIndex(defs, expr, option.index);
      return { ...switched, order: currentOrder(expr) };
    }
    default:
      return undefined;
  }
}

// Writes the canonical form: only applied indexed clauses, no padding.
export function normalizeFilters(expr: FilterExpression): FilterExpression {
  if (isDatabaseFilter(expr.index)) {
    return {
      ...expr,
      index: {
        name: expr.index.name,
        clauses: enabledIndexClauses(expr) as DatabaseIndexFilter["clauses"],
      },
    };
  }
  if (isSearchFilter(expr.index)) {
    return {
      ...expr,
      index: {
        ...expr.index,
        clauses: expr.index.clauses.filter((c) => c.enabled),
      },
    };
  }
  return expr;
}

export const scanOperatorOptions: Readonly<
  Option<(FilterByType | FilterByBuiltin)["op"]>[]
> = [
  { value: "eq", label: "equals" },
  { value: "neq", label: "does not equal" },
  { value: "gt", label: "is greater than" },
  { value: "gte", label: "is greater than or equal to" },
  { value: "lt", label: "is less than" },
  { value: "lte", label: "is less than or equal to" },
  { value: "type", label: "is type" },
  { value: "notype", label: "is not type" },
];

export type IndexedOperator = "eq" | "lt" | "lte" | "gt" | "gte" | "between";

export const indexedOperatorOptions: Readonly<Option<IndexedOperator>[]> = [
  { value: "eq", label: "equals" },
  { value: "gt", label: "is greater than" },
  { value: "gte", label: "is greater than or equal to" },
  { value: "lt", label: "is less than" },
  { value: "lte", label: "is less than or equal to" },
  { value: "between", label: "is between" },
];

export function indexedOperatorOf(
  clause: DatabaseIndexFilterClause,
): IndexedOperator {
  if (!isRangeClause(clause)) return "eq";
  if (clause.lowerOp && clause.upperOp) return "between";
  return clause.lowerOp ?? clause.upperOp ?? "between";
}

// Rebuilds a clause under a new operator, carrying the most relevant bound
// over so the user doesn't retype the value.
export function withIndexedOperator(
  clause: DatabaseIndexFilterClause,
  op: IndexedOperator,
): DatabaseIndexFilterClause {
  const lower = isRangeClause(clause) ? clause.lowerValue : clause.value;
  const upper = isRangeClause(clause) ? clause.upperValue : clause.value;
  const primary = lower ?? upper ?? null;
  switch (op) {
    case "eq":
      return { type: "indexEq", enabled: true, value: primary };
    case "between":
      return {
        type: "indexRange",
        enabled: true,
        lowerOp: "gte",
        lowerValue: lower ?? primary,
        upperOp: "lte",
        upperValue: upper ?? primary,
      };
    case "gt":
    case "gte":
      return {
        type: "indexRange",
        enabled: true,
        lowerOp: op,
        lowerValue: lower ?? primary,
      };
    case "lt":
    case "lte":
      return {
        type: "indexRange",
        enabled: true,
        upperOp: op,
        upperValue: upper ?? primary,
      };
    default:
      return clause;
  }
}

const OPERATOR_SYMBOLS: Record<string, string> = {
  eq: "=",
  neq: "≠",
  gt: ">",
  gte: "≥",
  lt: "<",
  lte: "≤",
  type: "is",
  notype: "is not",
};

function operatorSymbol(op: string): string {
  return OPERATOR_SYMBOLS[op] ?? op;
}

function formatDateLikeInput(date: Date): string {
  return date.toLocaleString(undefined, {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

// Text the user typed that doesn't parse into a value. It is kept in the draft
// filter under this key so the chip and a reopened editor still show what was
// typed; `hasUnparsedValue` keeps such a filter from ever being applied.
const UNPARSED_KEY = "$unparsed";

export function unparsedValue(text: string): JSONValue {
  return { [UNPARSED_KEY]: text };
}

export function unparsedText(
  value: JSONValue | Value | undefined,
): string | undefined {
  if (typeof value !== "object" || value === null) return undefined;
  const text = (value as Record<string, unknown>)[UNPARSED_KEY];
  return typeof text === "string" && Object.keys(value).length === 1
    ? text
    : undefined;
}

type ClauseWithValues = {
  value?: JSONValue | Value;
  lowerValue?: JSONValue | Value;
  upperValue?: JSONValue | Value;
};

export function hasUnparsedClauseValue(clause: ClauseWithValues): boolean {
  return [clause.value, clause.lowerValue, clause.upperValue].some(
    (value) => unparsedText(value) !== undefined,
  );
}

// Only the clauses a query would run on count: a disabled one is skipped
// there, and has no chip here, so text it holds would block applying with
// nothing on screen to fix. Scan clauses without `enabled` are enabled.
export function hasUnparsedValue(expr: FilterExpression): boolean {
  const indexClauses: ClauseWithValues[] = isSearchFilter(expr.index)
    ? expr.index.clauses.filter((c) => c.enabled)
    : enabledIndexClauses(expr);
  return (
    expr.clauses
      .filter((c) => c.enabled !== false)
      .some(hasUnparsedClauseValue) || indexClauses.some(hasUnparsedClauseValue)
  );
}

// The value editor can only hand back a serializable value, so an empty
// ("unset") input arrives as `UNDEFINED_PLACEHOLDER`; both mean unset.
export function formatFilterValue(
  field: string,
  value: JSONValue | Value | undefined,
): string {
  const unparsed = unparsedText(value);
  if (unparsed !== undefined) return unparsed;
  if (value === undefined || value === UNDEFINED_PLACEHOLDER) return "unset";
  if (field === "_creationTime" && typeof value === "number") {
    return formatDateLikeInput(new Date(value));
  }
  try {
    return stringifyValue(value as Value);
  } catch {
    return JSON.stringify(value);
  }
}

export function describeIndexedClause(
  field: string,
  clause: DatabaseIndexFilterClause,
): string {
  if (!isRangeClause(clause)) {
    return `${field} = ${formatFilterValue(field, clause.value)}`;
  }
  const op = indexedOperatorOf(clause);
  if (op === "between") {
    return `${field} between ${formatFilterValue(field, clause.lowerValue)} and ${formatFilterValue(field, clause.upperValue)}`;
  }
  const value = clause.lowerOp ? clause.lowerValue : clause.upperValue;
  return `${field} ${operatorSymbol(op)} ${formatFilterValue(field, value)}`;
}

export function describeScanClause(clause: Filter): string {
  const field = clause.field ?? "?";
  if (clause.op === "type" || clause.op === "notype") {
    return `${field} ${operatorSymbol(clause.op)} ${clause.value ?? "?"}`;
  }
  if (clause.op === "anyOf" || clause.op === "noneOf") {
    return `${field} ${clause.op}`;
  }
  return `${field} ${operatorSymbol(clause.op)} ${formatFilterValue(field, clause.value)}`;
}

// One line for history menus: "team = "eng" › status = "active" · newest first".
export function summarizeFilters(
  defs: IndexDef[],
  expr: FilterExpression,
): string {
  const parts: string[] = [];
  if (isSearchFilter(expr.index)) {
    const def = findIndexDef(defs, expr.index.name);
    const searchField = def?.kind === "search" ? def.searchField : "text";
    parts.push(`${searchField} ~ ${JSON.stringify(expr.index.search)}`);
    for (const c of expr.index.clauses.filter((c) => c.enabled)) {
      parts.push(`${c.field} = ${formatFilterValue(c.field, c.value)}`);
    }
  } else {
    const index = currentDatabaseIndex(defs, expr);
    parts.push(
      ...enabledIndexClauses(expr).map((c, i) =>
        describeIndexedClause(index.fields[i] ?? "?", c),
      ),
    );
  }
  const scan = expr.clauses
    .filter(isValidFilter)
    .filter((c) => c.enabled !== false)
    .map(describeScanClause);
  const clauseText = [parts.join(" › "), scan.join(", ")]
    .filter((text) => text.length > 0)
    .join(" · ");
  if (isSearchFilter(expr.index)) return clauseText;
  const sortField = effectiveSortField(defs, expr);
  const order = currentOrder(expr);
  const sortText =
    sortField === "_creationTime"
      ? order === "desc"
        ? "newest first"
        : "oldest first"
      : `${sortField} ${order === "desc" ? "↓" : "↑"}`;
  return clauseText.length > 0 ? `${clauseText} · ${sortText}` : sortText;
}

export function defaultIndexedClause(
  field: string,
  defaultDocument: GenericDocument,
): DatabaseIndexFilterClause {
  if (field === "_creationTime") {
    return {
      type: "indexRange",
      enabled: true,
      upperOp: "lte",
      upperValue: Date.now(),
    };
  }
  return {
    type: "indexEq",
    enabled: true,
    value:
      field === "_id"
        ? ""
        : defaultDocument[field] === undefined
          ? UNDEFINED_PLACEHOLDER
          : convexToJson(defaultDocument[field]),
  };
}

// The same seed as an indexed clause, reduced to the value a search clause can
// hold: they filter by equality only, so a field whose sample value has no
// equality form starts unset.
export function defaultSearchFilterValue(
  field: string,
  defaultDocument: GenericDocument,
): JSONValue | undefined {
  const clause = defaultIndexedClause(field, defaultDocument);
  return clause.type === "indexEq" ? (clause.value as JSONValue) : undefined;
}

export function newClauseId(): string {
  return Math.random().toString(36).slice(2);
}

export function defaultScanClause(
  field: string,
  defaultDocument: GenericDocument,
): Filter {
  if (field === "_creationTime") {
    return {
      id: newClauseId(),
      field,
      op: "lte",
      value: Date.now(),
      enabled: true,
    };
  }
  return {
    id: newClauseId(),
    field,
    op: "eq",
    value:
      field === "_id"
        ? ""
        : defaultDocument[field] === undefined
          ? UNDEFINED_PLACEHOLDER
          : convexToJson(defaultDocument[field]),
    enabled: true,
  };
}
