import { GenericDocument } from "convex/server";
import { JSONValue } from "convex/values";
import isEqual from "lodash/isEqual";
import {
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  DatabaseIndexFilterClause,
  Filter,
  FilterExpression,
  SearchIndexFilterClause,
} from "system-udfs/convex/_system/frontend/lib/filters";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import {
  DatabaseIndexDef,
  EMPTY_FILTERS,
  FieldOption,
  IndexDef,
  NextIndexedField,
  SearchIndexDef,
  SortOption,
  addIndexedClause,
  addScanClause,
  addSearchFilterClause,
  clearSearchIndex,
  currentDatabaseIndex,
  defaultIndexedClause,
  defaultScanClause,
  defaultSearchFilterValue,
  enabledIndexClauses,
  findIndexDef,
  hasUnparsedClauseValue,
  hasUnparsedValue,
  isSearchFilter,
  newClauseId,
  nextIndexedFields,
  nextSearchFilterFields,
  normalizeFilters,
  optionsForField,
  removeIndexedClause,
  removeScanClause,
  removeSearchFilterClause,
  setDatabaseIndex,
  setIndexedClause,
  setOrder as setOrderInModel,
  setScanClause,
  setSearchFilterClause,
  setSearchIndex,
  setSearchText,
  sortByField as sortByFieldInModel,
  sortOptionForField,
  switchIndexAndAddClause,
  unparsedText,
} from "./filterModel";

export type FilterItem =
  | {
      key: string;
      kind: "indexed";
      position: number;
      field: string;
      clause: DatabaseIndexFilterClause;
      isLast: boolean;
    }
  | { key: string; kind: "scan"; position: number; clause: Filter }
  | { key: string; kind: "search"; field: string; search: string }
  | {
      key: string;
      kind: "searchFilter";
      position: number;
      clause: SearchIndexFilterClause;
    };

// A chip's error, and whether it is the user's to see yet: an error about a
// value an editor was seeded with blocks the filter from being applied but
// isn't marked until the first edit.
export type FilterItemError = { message: string; shown: boolean };

export type FilterItemUpdate =
  | { kind: "indexed"; clause: DatabaseIndexFilterClause }
  | { kind: "scan"; clause: Filter }
  | { kind: "search"; search: string }
  | { kind: "searchFilter"; value: JSONValue | undefined };

export type FilterActions = ReturnType<typeof useFilterActions>;

const scanKey = (clause: Filter, position: number) =>
  `scan/${clause.id ?? position}`;

const searchFilterKey = (field: string) => `searchFilter/${field}`;

function buildFilterItems(
  defs: IndexDef[],
  expr: FilterExpression,
): FilterItem[] {
  const filterItems: FilterItem[] = [];
  if (isSearchFilter(expr.index)) {
    const def = findIndexDef(defs, expr.index.name);
    filterItems.push({
      key: "search",
      kind: "search",
      field: def?.kind === "search" ? def.searchField : expr.index.name,
      search: expr.index.search,
    });
    expr.index.clauses
      .filter((c) => c.enabled)
      .forEach((clause, position) =>
        filterItems.push({
          // Keyed by field, not position: a search index takes each filter
          // field at most once, so this identity survives the reindexing that
          // removing an earlier clause would otherwise do to it.
          key: searchFilterKey(clause.field),
          kind: "searchFilter",
          position,
          clause,
        }),
      );
  } else {
    const index = currentDatabaseIndex(defs, expr);
    const used = enabledIndexClauses(expr);
    used.forEach((clause, position) =>
      filterItems.push({
        key: `indexed/${position}`,
        kind: "indexed",
        position,
        field: index.fields[position] ?? "?",
        clause,
        isLast: position === used.length - 1,
      }),
    );
  }
  expr.clauses.forEach((clause, position) =>
    filterItems.push({
      key: scanKey(clause, position),
      kind: "scan",
      position,
      clause,
    }),
  );
  return filterItems;
}

// Whether the update carries text the editor couldn't parse rather than a
// value.
function isUnparsedUpdate(update: FilterItemUpdate): boolean {
  switch (update.kind) {
    case "indexed":
    case "scan":
      return hasUnparsedClauseValue(update.clause);
    case "searchFilter":
      return unparsedText(update.value) !== undefined;
    default:
      return false;
  }
}

// How long to wait after the last keystroke in a value editor before
// running the query with the new value.
const VALUE_APPLY_DEBOUNCE_MS = 400;

// Owns the draft/applied split for the filter bar and turns user intents
// (add the index's next field, add a scan, remove a chip, sort by a
// column, ...) into valid filter expressions. Everything applies as soon as
// it is valid; typed values are debounced so each keystroke doesn't run a
// query.
export function useFilterActions({
  filters,
  draftFilters,
  setDraftFilters,
  applyFilters,
  indexDefs,
  defaultDocument,
}: {
  filters: FilterExpression | undefined;
  draftFilters: FilterExpression | undefined;
  setDraftFilters(next: FilterExpression | undefined): void;
  applyFilters(next: FilterExpression): Promise<void> | void;
  indexDefs: IndexDef[];
  defaultDocument: GenericDocument;
}) {
  const { useLogDeploymentEvent } = useContext(DeploymentInfoContext);
  const log = useLogDeploymentEvent();

  const applied = useMemo(() => filters ?? EMPTY_FILTERS, [filters]);
  const shown = useMemo(() => draftFilters ?? applied, [draftFilters, applied]);

  const apply = useCallback(
    (next: FilterExpression) => {
      setDraftFilters(next);
      void applyFilters(normalizeFilters(next));
    },
    [applyFilters, setDraftFilters],
  );

  const [openFilterItemKey, setOpenFilterItemKey] = useState<string | null>(
    null,
  );
  const [errors, setErrors] = useState<Record<string, FilterItemError>>({});
  // Mirrors `errors` for the debounced apply, which runs outside a render and
  // so can't read this render's state. Written only by `setError`, so the two
  // never drift.
  const errorsRef = useRef(errors);

  const setError = useCallback(
    (key: string, error: FilterItemError | undefined) => {
      const prev = errorsRef.current;
      if (isEqual(prev[key], error)) return;
      const next = { ...prev };
      if (error === undefined) {
        delete next[key];
      } else {
        next[key] = error;
      }
      errorsRef.current = next;
      setErrors(next);
    },
    [],
  );

  const pendingApply = useRef<{
    timer: ReturnType<typeof setTimeout>;
    next: FilterExpression;
  } | null>(null);
  useEffect(
    () => () => {
      if (pendingApply.current) clearTimeout(pendingApply.current.timer);
    },
    [],
  );

  const filterItems = useMemo(
    () => buildFilterItems(indexDefs, shown),
    [indexDefs, shown],
  );
  const hasInvalid =
    filterItems.some((c) => errors[c.key]) || hasUnparsedValue(shown);

  // An error lives exactly as long as the item that reported it. Indexed
  // chips are keyed by position, so a key freed by one edit can come back
  // pointing at a different field (switching index, then filtering by the new
  // index's first field); forgetting the error the moment its item goes is
  // what stops it resurfacing on the replacement.
  useEffect(() => {
    const live = new Set(filterItems.map((c) => c.key));
    Object.keys(errorsRef.current).forEach((key) => {
      if (!live.has(key)) setError(key, undefined);
    });
  }, [filterItems, setError]);

  // Only an editor reporting a problem writes an error. Clearing belongs to
  // `updateFilterItem`, which runs when an editor parses a value, so an
  // `onError([])` from an editor unmounting can't wipe the error the closed
  // chip is there to show.
  const setFilterItemError = useCallback(
    (key: string, messages: string[], isShown = true) => {
      if (messages.length === 0) return;
      // An error the user has already been shown stays shown, so remounting
      // the editor (reopening the chip, switching operator) doesn't quiet it.
      setError(key, {
        message: messages[0],
        shown: isShown || errorsRef.current[key]?.shown === true,
      });
    },
    [setError],
  );

  // Text that never parsed counts even without an error to go with it: it is
  // not a value, so a filter holding one can't run.
  const hasErrorsFor = useCallback(
    (expr: FilterExpression) =>
      hasUnparsedValue(expr) ||
      buildFilterItems(indexDefs, expr).some((c) => errorsRef.current[c.key]),
    [indexDefs],
  );

  const scheduleApply = useCallback(
    (next: FilterExpression) => {
      if (pendingApply.current) clearTimeout(pendingApply.current.timer);
      const timer = setTimeout(() => {
        pendingApply.current = null;
        if (!hasErrorsFor(next)) apply(next);
      }, VALUE_APPLY_DEBOUNCE_MS);
      pendingApply.current = { timer, next };
    },
    [apply, hasErrorsFor],
  );

  const flushPendingApply = useCallback(() => {
    if (!pendingApply.current) return;
    clearTimeout(pendingApply.current.timer);
    const { next } = pendingApply.current;
    pendingApply.current = null;
    if (!hasErrorsFor(next)) apply(next);
  }, [apply, hasErrorsFor]);

  // Structural edits go live right away unless a chip still holds an
  // unparseable value, in which case they stay in the draft.
  const commitLive = useCallback(
    (next: FilterExpression) => {
      if (hasInvalid) {
        setDraftFilters(next);
      } else {
        apply(next);
      }
    },
    [apply, hasInvalid, setDraftFilters],
  );

  const indexedNext: NextIndexedField[] = useMemo(
    () => nextIndexedFields(indexDefs, shown),
    [indexDefs, shown],
  );
  const searchFilterNext = useMemo(
    () => nextSearchFilterFields(indexDefs, shown),
    [indexDefs, shown],
  );
  const searchActive = isSearchFilter(shown.index);
  const used = enabledIndexClauses(shown);
  const currentIndex = currentDatabaseIndex(indexDefs, shown);

  // Every add lands the same way: the new item's editor opens, and the apply
  // goes through the debounce so that editor can mount and report an invalid
  // initial value (e.g. "" for an `_id` field) before the query runs.
  const addAndOpen = useCallback(
    (
      next: FilterExpression,
      itemKey: string,
      logProps: Record<string, string>,
    ) => {
      setDraftFilters(next);
      scheduleApply(next);
      setOpenFilterItemKey(itemKey);
      log("filter add", logProps);
    },
    [log, scheduleApply, setDraftFilters, setOpenFilterItemKey],
  );

  // Adds the index's next clause on `field`, seeded from a sample document
  // so results appear right away. Only the clause each index kind takes
  // differs; adding it is the same either way.
  const addField = useCallback(
    (field: string) => {
      let added:
        | { expr: FilterExpression; itemKey: string; filterType: string }
        | undefined;
      if (searchActive) {
        if (searchFilterNext.includes(field)) {
          added = {
            expr: addSearchFilterClause(
              shown,
              field,
              defaultSearchFilterValue(field, defaultDocument),
            ),
            itemKey: searchFilterKey(field),
            filterType: "searchFilter",
          };
        }
      } else if (indexedNext.some((n) => n.field === field)) {
        added = {
          expr: addIndexedClause(
            indexDefs,
            shown,
            field,
            defaultIndexedClause(field, defaultDocument),
          ),
          itemKey: `indexed/${used.length}`,
          filterType: "index",
        };
      }
      if (!added) return false;
      addAndOpen(added.expr, added.itemKey, { filterType: added.filterType });
      return true;
    },
    [
      addAndOpen,
      defaultDocument,
      indexDefs,
      indexedNext,
      searchActive,
      searchFilterNext,
      shown,
      used.length,
    ],
  );

  // Adds an unindexed clause on `field` on top of whatever the index
  // returns, and opens its editor.
  const addScanField = useCallback(
    (field: string) => {
      const clause = defaultScanClause(field, defaultDocument);
      addAndOpen(
        addScanClause(shown, clause),
        scanKey(clause, shown.clauses.length),
        { filterType: "regular" },
      );
    },
    [addAndOpen, defaultDocument, shown],
  );

  // Switches to `index` (keeping the applied clauses it shares) and adds an
  // indexed clause on `field`.
  const addFieldWithIndex = useCallback(
    (index: DatabaseIndexDef, field: string) => {
      const next = switchIndexAndAddClause(
        indexDefs,
        shown,
        index,
        field,
        defaultIndexedClause(field, defaultDocument),
      );
      addAndOpen(next, `indexed/${enabledIndexClauses(next).length - 1}`, {
        filterType: "index",
        switchedIndex: index.name,
      });
    },
    [addAndOpen, defaultDocument, indexDefs, shown],
  );

  const optionsFor = useCallback(
    (field: string): FieldOption[] => optionsForField(indexDefs, shown, field),
    [indexDefs, shown],
  );

  // Adds a complete clause from a cell's context menu and applies it: through
  // the index when the field is its next one, otherwise as a scan on top.
  const addComplete = useCallback(
    (filter: Filter): boolean => {
      const { field, op } = filter;
      if (field === undefined) return false;
      if (searchActive) {
        if (op !== "eq" || !searchFilterNext.includes(field)) return false;
        commitLive(
          addSearchFilterClause(shown, field, filter.value as JSONValue),
        );
        log("filter add", { filterType: "searchFilter", source: "table" });
        return true;
      }
      const value = filter.value as JSONValue | undefined;
      if (indexedNext.some((n) => n.field === field)) {
        let clause: DatabaseIndexFilterClause | undefined;
        switch (op) {
          case "eq":
            clause = { type: "indexEq", enabled: true, value };
            break;
          case "gt":
          case "gte":
            clause = {
              type: "indexRange",
              enabled: true,
              lowerOp: op,
              lowerValue: value,
            };
            break;
          case "lt":
          case "lte":
            clause = {
              type: "indexRange",
              enabled: true,
              upperOp: op,
              upperValue: value,
            };
            break;
          default:
            clause = undefined;
        }
        if (clause) {
          commitLive(addIndexedClause(indexDefs, shown, field, clause));
          log("filter add", { filterType: "index", source: "table" });
          return true;
        }
      }
      commitLive(
        addScanClause(shown, {
          ...filter,
          id: filter.id ?? newClauseId(),
          enabled: true,
        } as Filter),
      );
      log("filter add", { filterType: "regular", source: "table" });
      return true;
    },
    [
      commitLive,
      indexDefs,
      indexedNext,
      log,
      searchActive,
      searchFilterNext,
      shown,
    ],
  );

  const updateFilterItem = useCallback(
    (key: string, update: FilterItemUpdate) => {
      const filterItem = filterItems.find((c) => c.key === key);
      if (!filterItem) return;
      let next: FilterExpression | undefined;
      if (filterItem.kind === "indexed" && update.kind === "indexed") {
        next = setIndexedClause(
          indexDefs,
          shown,
          filterItem.position,
          update.clause,
        );
      } else if (filterItem.kind === "scan" && update.kind === "scan") {
        next = setScanClause(shown, filterItem.position, update.clause);
      } else if (filterItem.kind === "search" && update.kind === "search") {
        next = setSearchText(shown, update.search);
      } else if (
        filterItem.kind === "searchFilter" &&
        update.kind === "searchFilter"
      ) {
        next = setSearchFilterClause(shown, filterItem.position, update.value);
      }
      // Editors report their initial value on mount; that isn't a change and
      // must not restart the debounce.
      if (next && !isEqual(next, shown)) {
        // An editor reports a value only once it has parsed one, so the
        // change itself clears the chip's error. The editor reports onError
        // before onChange, so waiting for the next onError([]) would leave
        // the error — and with it the block on applying — in place until
        // the user typed again. Text that didn't parse arrives through the
        // same callback and is the error, so it leaves the error alone.
        if (!isUnparsedUpdate(update)) setError(key, undefined);
        setDraftFilters(next);
        scheduleApply(next);
      }
    },
    [filterItems, indexDefs, scheduleApply, setDraftFilters, setError, shown],
  );

  // Indexed filters (and the search a search index's filters hang off) are
  // removed from the end only, so no removal ever takes other clauses with it.
  const removeFilterItem = useCallback(
    (key: string) => {
      const filterItem = filterItems.find((c) => c.key === key);
      if (!filterItem) return;
      let next: FilterExpression | undefined;
      switch (filterItem.kind) {
        case "indexed": {
          if (!filterItem.isLast) return;
          const { expr } = removeIndexedClause(
            indexDefs,
            shown,
            filterItem.position,
          );
          next = expr;
          log("filter delete", { filterType: "index" });
          break;
        }
        case "scan":
          next = removeScanClause(shown, filterItem.position);
          log("filter delete", { filterType: "regular" });
          break;
        case "search":
          if (
            isSearchFilter(shown.index) &&
            shown.index.clauses.some((c) => c.enabled)
          ) {
            return;
          }
          next = clearSearchIndex(shown);
          log("filter delete", { filterType: "search" });
          break;
        case "searchFilter":
          next = removeSearchFilterClause(shown, filterItem.position);
          log("filter delete", { filterType: "searchFilter" });
          break;
        default:
          return;
      }
      if (!next) return;
      setError(key, undefined);
      if (openFilterItemKey === key) setOpenFilterItemKey(null);
      // Not `commitLive`: it judges this render's items, which still include
      // the one just removed. Only errors that survive the removal count.
      if (hasErrorsFor(next)) {
        setDraftFilters(next);
      } else {
        apply(next);
      }
    },
    [
      apply,
      filterItems,
      hasErrorsFor,
      indexDefs,
      log,
      openFilterItemKey,
      setDraftFilters,
      setError,
      setOpenFilterItemKey,
      shown,
    ],
  );

  const startSearch = useCallback(
    (def: SearchIndexDef) => {
      commitLive(setSearchIndex(shown, def));
      setOpenFilterItemKey("search");
      log("sort by index combobox opened", { selectedOption: def.name });
    },
    [commitLive, log, setOpenFilterItemKey, shown],
  );

  const chooseIndex = useCallback(
    (def: DatabaseIndexDef) => {
      commitLive(setDatabaseIndex(indexDefs, shown, def));
      log("sort by index combobox opened", { selectedOption: def.name });
    },
    [commitLive, indexDefs, log, shown],
  );

  const setOrder = useCallback(
    (order: "asc" | "desc") => {
      log("filter order change", { oldOrder: shown.order, newOrder: order });
      commitLive(setOrderInModel(shown, order));
    },
    [commitLive, log, shown],
  );

  const sortByField = useCallback(
    (field: string) => {
      const next = sortByFieldInModel(indexDefs, shown, field);
      if (!next) return false;
      log("filter order change", { source: "header", field });
      commitLive(next);
      return true;
    },
    [commitLive, indexDefs, log, shown],
  );

  const sortOptionFor = useCallback(
    (field: string): SortOption => sortOptionForField(indexDefs, shown, field),
    [indexDefs, shown],
  );

  // Closing a chip's editor flushes any value still waiting on the debounce.
  const closeFilterItem = useCallback(() => {
    setOpenFilterItemKey(null);
    flushPendingApply();
  }, [flushPendingApply, setOpenFilterItemKey]);

  return {
    shown,
    filterItems,
    errors,
    openFilterItemKey,
    setOpenFilterItemKey,
    closeFilterItem,
    setFilterItemError,
    indexedNext,
    searchFilterNext,
    searchActive,
    currentIndex,
    addField,
    addScanField,
    addFieldWithIndex,
    optionsFor,
    addComplete,
    updateFilterItem,
    removeFilterItem,
    startSearch,
    chooseIndex,
    setOrder,
    sortByField,
    sortOptionFor,
  };
}
