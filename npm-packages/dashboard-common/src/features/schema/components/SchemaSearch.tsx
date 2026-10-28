import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { FixedSizeList } from "react-window";
import { filter as fuzzyFilter } from "fuzzy";
import {
  MagnifyingGlassIcon,
  Cross2Icon,
  TableIcon,
  BoxModelIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { cn } from "@ui/cn";
import {
  SchemaGraph,
  SchemaIndex,
} from "@common/features/schema/lib/buildSchemaGraph";
import { Cluster } from "@common/features/schema/lib/clustering";
import { isTextEditingTarget } from "@common/lib/isTextEditingTarget";
import { IndexIcon, FieldIcon, INDEX_KIND_LABEL } from "@common/elements/icons";

const SEARCH_HOTKEY = "?";
const RESULT_ROW_HEIGHT = 32;
const MAX_VISIBLE_RESULTS = 8;

export type SchemaSearchEntry =
  // A cluster carries all its member tables (so picking it can frame the whole
  // group) plus a representative `table` for the plain pan fallback.
  | {
      kind: "cluster";
      table: string;
      tables: string[];
      label: string;
      name: string;
    }
  | { kind: "table"; table: string; label: string; name: string }
  | { kind: "field"; table: string; label: string; name: string }
  | {
      kind: "index";
      table: string;
      indexKind: SchemaIndex["kind"];
      label: string;
      name: string;
    };

export function buildSearchEntries(
  graph: SchemaGraph,
  clusters: Cluster[] = [],
): SchemaSearchEntry[] {
  const entries: SchemaSearchEntry[] = [];
  // Clusters first, so they surface at the top of the results. A lone cluster
  // spans the whole diagram (and isn't drawn), so it's not offered in search.
  if (clusters.length >= 2) {
    clusters.forEach((cluster) => {
      entries.push({
        kind: "cluster",
        table: cluster.tables[0],
        tables: cluster.tables,
        label: cluster.label,
        name: cluster.label,
      });
    });
  }
  graph.nodes.forEach((node) => {
    entries.push({
      kind: "table",
      table: node.table,
      label: node.table,
      name: node.table,
    });
    node.fields.forEach((field) => {
      entries.push({
        kind: "field",
        table: node.table,
        label: `${node.table}.${field.name}`,
        name: field.name,
      });
    });
    node.indexes.forEach((index) => {
      entries.push({
        kind: "index",
        table: node.table,
        indexKind: index.kind,
        label: `${node.table}.${index.name}`,
        name: index.name,
      });
    });
  });
  return entries;
}

function matchIndices(query: string, text: string): Set<number> {
  const indices = new Set<number>();
  const pattern = query.toLowerCase();
  const lower = text.toLowerCase();
  let p = 0;
  for (let i = 0; i < lower.length && p < pattern.length; i += 1) {
    if (lower[i] === pattern[p]) {
      indices.add(i);
      p += 1;
    }
  }
  return indices;
}

function highlightMatch(text: string, query: string): ReactNode {
  if (query.trim() === "") {
    return text;
  }
  const indices = matchIndices(query, text);
  if (indices.size === 0) {
    return text;
  }
  const parts: ReactNode[] = [];
  let buffer = "";
  let bufferHighlighted = false;
  let key = 0;
  const flush = () => {
    if (!buffer) return;
    parts.push(
      bufferHighlighted ? (
        <span key={key} className="font-semibold text-content-accent">
          {buffer}
        </span>
      ) : (
        <span key={key}>{buffer}</span>
      ),
    );
    key += 1;
    buffer = "";
  };
  for (let i = 0; i < text.length; i += 1) {
    const highlighted = indices.has(i);
    if (highlighted !== bufferHighlighted) {
      flush();
      bufferHighlighted = highlighted;
    }
    buffer += text[i];
  }
  flush();
  return parts;
}

function EntryIcon({ entry }: { entry: SchemaSearchEntry }) {
  let icon: ReactNode;
  let title: string;
  if (entry.kind === "cluster") {
    icon = <BoxModelIcon className="size-3.5 text-content-secondary" />;
    title = "Group";
  } else if (entry.kind === "table") {
    icon = <TableIcon className="size-3.5 text-content-secondary" />;
    title = "Table";
  } else if (entry.kind === "field") {
    icon = <FieldIcon className="text-sm text-content-secondary" />;
    title = "Field";
  } else {
    icon = (
      <IndexIcon kind={entry.indexKind} className="text-content-secondary" />
    );
    title = INDEX_KIND_LABEL[entry.indexKind];
  }
  return (
    <span
      title={title}
      className="flex size-4 shrink-0 items-center justify-center"
    >
      {icon}
    </span>
  );
}

type RowData = {
  matches: SchemaSearchEntry[];
  activeIndex: number;
  query: string;
  // True when the query has a ".", so the whole `table.name` label matched (and
  // should be highlighted) rather than just the field/index name.
  matchLabel: boolean;
  pick: (entry: SchemaSearchEntry) => void;
  setActiveIndex: (index: number) => void;
};

const ResultRow = memo(function ResultRow({
  index,
  style,
  data,
}: {
  index: number;
  style: React.CSSProperties;
  data: RowData;
}) {
  const { matches, activeIndex, query, matchLabel, pick, setActiveIndex } =
    data;
  const entry = matches[index];
  // Everything before the matched name (e.g. `table.`) is context, shown plain —
  // unless the query matched the full label (has a "."), where the prefix can
  // match too and should be highlighted.
  const prefix = entry.label.slice(0, entry.label.length - entry.name.length);
  return (
    // eslint-disable-next-line react/forbid-elements -- a search-result row needs onMouseDown/onMouseEnter and full-width left-aligned layout that @ui/Button doesn't model
    <button
      type="button"
      style={style}
      // Keep focus on the input so onBlur doesn't close the list first.
      onMouseDown={(e) => e.preventDefault()}
      onClick={() => pick(entry)}
      onMouseEnter={() => setActiveIndex(index)}
      className={cn(
        "flex w-full items-center gap-1.5 px-2.5 text-left text-sm",
        index === activeIndex && "bg-background-tertiary",
      )}
    >
      <EntryIcon entry={entry} />
      <span className="truncate font-mono text-content-primary">
        {matchLabel ? (
          highlightMatch(entry.label, query)
        ) : (
          <>
            {prefix}
            {highlightMatch(entry.name, query)}
          </>
        )}
      </span>
    </button>
  );
});

// Floating search box (top-left). Fuzzy-matches every table, field, and index;
// picking a result reveals and pans to its table. The result list is
// virtualized, so it stays fast no matter how many entries there are.
export function SchemaSearch({
  entries,
  onPick,
  onPickCluster,
  onOpenChange,
}: {
  entries: SchemaSearchEntry[];
  onPick: (table: string) => void;
  // Frame a whole group when a cluster result is picked.
  onPickCluster: (tables: string[]) => void;
  // Notified when the dropdown opens/closes (used to suppress the minimap).
  onOpenChange: (open: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState(false);
  const [activeIndex, setActiveIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<FixedSizeList>(null);

  // Set open state and notify the parent in one call, so every open/close path
  // stays in sync without an effect mirroring `open` back out.
  const setOpenAndNotify = useCallback(
    (next: boolean) => {
      setOpen(next);
      onOpenChange(next);
    },
    [onOpenChange],
  );

  // Change the query and highlight the first result. Resetting here (rather than
  // in an effect keyed on `query`) keeps every query-change path in sync in one
  // place — the result set only changes when the query does.
  const changeQuery = useCallback((next: string) => {
    setQuery(next);
    setActiveIndex(0);
  }, []);

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (
        event.key !== SEARCH_HOTKEY ||
        event.metaKey ||
        event.ctrlKey ||
        event.altKey
      ) {
        return;
      }
      // Don't steal the hotkey while the user is typing in a field.
      if (isTextEditingTarget(document.activeElement)) {
        return;
      }
      event.preventDefault();
      setOpenAndNotify(true);
      inputRef.current?.focus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [setOpenAndNotify]);

  const matches = useMemo<SchemaSearchEntry[]>(() => {
    if (query.trim() === "") {
      // No query: list groups and tables (fields/indexes would be too noisy).
      return entries.filter((e) => e.kind === "cluster" || e.kind === "table");
    }
    // A "." means the user is qualifying `table.name`, so match against the full
    // `table.name` label (surfacing that table's fields/indexes). Otherwise match
    // names only, so a field/index surfaces just when the query hits its own name
    // — not when it only matches the `table.` prefix.
    const byLabel = query.includes(".");
    const ranked = fuzzyFilter(query, entries, {
      extract: (e) => (byLabel ? e.label : e.name),
    }).map((result) => result.original);
    // Prioritize groups, then tables, then fields, then indexes. Array.sort is
    // stable, so fuzzy's relevance order is preserved within each kind.
    const rank = { cluster: 0, table: 1, field: 2, index: 3 };
    return ranked.sort((a, b) => rank[a.kind] - rank[b.kind]);
  }, [query, entries]);

  // Keep the highlighted row scrolled into view as it moves.
  useEffect(() => {
    if (open) {
      listRef.current?.scrollToItem(activeIndex);
    }
  }, [activeIndex, open]);

  const pick = useCallback(
    (entry: SchemaSearchEntry) => {
      if (entry.kind === "cluster") {
        onPickCluster(entry.tables);
      } else {
        onPick(entry.table);
      }
      changeQuery("");
      setOpenAndNotify(false);
      inputRef.current?.blur();
    },
    [onPick, onPickCluster, setOpenAndNotify, changeQuery],
  );

  // Stable so react-window only re-renders rows whose data actually changed
  // (rather than every visible row on each parent render — which made
  // scrolling janky).
  const itemData = useMemo<RowData>(
    () => ({
      matches,
      activeIndex,
      query,
      matchLabel: query.includes("."),
      pick,
      setActiveIndex,
    }),
    [matches, activeIndex, query, pick],
  );

  const showList = open && matches.length > 0;
  const listHeight =
    Math.min(matches.length, MAX_VISIBLE_RESULTS) * RESULT_ROW_HEIGHT;

  return (
    <div className="absolute top-3 left-3 z-10">
      <div className="flex w-72 items-center gap-1.5 rounded-lg border bg-background-secondary px-2 shadow-sm">
        <MagnifyingGlassIcon className="size-3.5 shrink-0 text-content-secondary" />
        <input
          ref={inputRef}
          value={query}
          placeholder={`${SEARCH_HOTKEY} to search`}
          aria-label="Search groups, tables, fields, and indexes"
          className="h-8 w-full bg-transparent text-sm text-content-primary placeholder:text-content-tertiary focus:outline-none"
          onChange={(e) => {
            changeQuery(e.target.value);
            setOpenAndNotify(true);
          }}
          onFocus={() => setOpenAndNotify(true)}
          // Delay close so a click on a result still registers before unmount.
          onBlur={() => window.setTimeout(() => setOpenAndNotify(false), 120)}
          // Don't let keystrokes reach React Flow's canvas handlers while typing.
          onKeyDown={(e) => {
            e.stopPropagation();
            if (e.key === "ArrowDown") {
              e.preventDefault();
              setOpenAndNotify(true);
              setActiveIndex(Math.min(activeIndex + 1, matches.length - 1));
            } else if (e.key === "ArrowUp") {
              e.preventDefault();
              setActiveIndex(Math.max(activeIndex - 1, 0));
            } else if (e.key === "Enter") {
              e.preventDefault();
              const match = matches[activeIndex];
              if (match) {
                pick(match);
              }
            } else if (e.key === "Escape") {
              e.preventDefault();
              if (query) {
                changeQuery("");
              } else {
                setOpenAndNotify(false);
                inputRef.current?.blur();
              }
            }
          }}
        />
        {query && (
          <Button
            size="xs"
            variant="neutral"
            inline
            className="border border-transparent"
            icon={<Cross2Icon />}
            aria-label="Clear search"
            onClick={() => {
              changeQuery("");
              inputRef.current?.focus();
            }}
          />
        )}
      </div>
      {showList && (
        <div className="mt-1 w-96 max-w-[calc(100vw-1.5rem)] overflow-hidden rounded-lg border bg-background-secondary py-1 shadow-sm">
          <FixedSizeList
            ref={listRef}
            height={listHeight}
            width="100%"
            itemCount={matches.length}
            itemSize={RESULT_ROW_HEIGHT}
            itemData={itemData}
            overscanCount={6}
            className="scrollbar"
          >
            {ResultRow}
          </FixedSizeList>
        </div>
      )}
    </div>
  );
}
