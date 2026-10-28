import {
  MouseEvent,
  useContext,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  ChevronDownIcon,
  ChevronRightIcon,
  MagnifyingGlassIcon,
} from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { Link } from "@ui/Link";
import { Popover } from "@ui/Popover";
import { Tooltip } from "@ui/Tooltip";
import { cn } from "@ui/cn";
import { useIsNarrowScreen } from "@ui/useIsNarrowScreen";
import { PuzzlePieceIcon } from "@common/elements/icons";
import { NENT_APP_PLACEHOLDER, Nent, useNents } from "@common/lib/useNents";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import {
  ancestorPaths,
  buildNentTree,
  expandableNentPaths,
  isUnmounted,
  NentTreeRow,
  subtreeRowCounts,
  visibleNentRows,
} from "@common/elements/nentTree";

/**
 * Clicking a component's name opens it. The toggle before it, a full-height
 * square with its own hover state, expands it; the highlight on the name never
 * covers the toggle, so the two click targets read as separate. Below an
 * expanded component, a rail runs alongside its children and collapses it when
 * clicked. A button at the end of the search bar expands or collapses every
 * component at once.
 */
export function NentSwitcherTree({
  onChange,
  className,
}: {
  onChange?: (nent: string | null) => void;
  className?: string;
}) {
  const { nents: allNents, selectedNent, setSelectedNent } = useNents();
  const { deploymentsURI } = useContext(DeploymentInfoContext);
  const isNarrow = useIsNarrowScreen();
  if (!allNents || allNents.length <= 1) {
    return null;
  }
  // The list from the server includes the root component, which has no name;
  // `useNents` stands for it with the app placeholder.
  const nents = allNents.filter(
    (nent) => nent.path === NENT_APP_PLACEHOLDER || nent.name !== null,
  );

  return (
    <div className={className}>
      <Popover
        placement="bottom-start"
        offset={[0, 4]}
        portal
        padding={false}
        sheetOnNarrowScreens
        className="w-72 overflow-hidden"
        // Headless UI hands its props (such as `aria-expanded`) to this button
        // directly, so it must stay an element rather than a wrapper component.
        asChild
        button={({ open }) => (
          <Button
            variant="unstyled"
            tip="Switch between components installed in this deployment."
            tipSide="right"
            className={cn(
              "flex w-full items-center gap-1 truncate rounded-md border bg-background-secondary p-1.5 text-left text-sm text-content-primary",
              "hover:bg-background-tertiary focus-visible:border-border-selected focus-visible:outline-hidden",
              open && "border-border-selected",
              selectedNent &&
                "bg-yellow-100/50 hover:bg-yellow-100 dark:bg-yellow-600/20 dark:hover:bg-yellow-600/50",
            )}
          >
            <PuzzlePieceIcon aria-hidden className="mt-px min-w-[13px]" />
            <span className="truncate">
              {/* Names the button by its selection, which is what it shows. */}
              <span className="sr-only">Component: </span>
              {selectedNent ? selectedNent.path : "app"}
              {selectedNent && isUnmounted(selectedNent) && (
                <>
                  <span aria-hidden>*</span>
                  <span className="sr-only"> (unmounted)</span>
                </>
              )}
            </span>
            <ChevronDownIcon
              className={cn(
                "ml-auto size-4 shrink-0 transition-transform",
                open && "rotate-180",
              )}
            />
          </Button>
        )}
      >
        {({ close }) => (
          <TreePanel
            nents={nents}
            selectedPath={selectedNent?.path ?? NENT_APP_PLACEHOLDER}
            isNarrow={isNarrow}
            onSelect={(nent) => {
              const id = nent.path === NENT_APP_PLACEHOLDER ? null : nent.id;
              void setSelectedNent(id ?? undefined);
              onChange?.(id);
              close();
            }}
          />
        )}
      </Popover>
      {selectedNent && isUnmounted(selectedNent) && (
        <p className="mt-1 text-xs text-content-secondary">
          This component is unmounted. You can delete it and its data on the{" "}
          <Link passHref href={`${deploymentsURI}/settings/components`}>
            Components settings
          </Link>{" "}
          page.
        </p>
      )}
    </div>
  );
}

// Mark the toggle, the name and the rail, so one click handler on the row can
// tell expanding and collapsing apart from opening. Clicks elsewhere in the row
// (the indentation) do nothing.
const TOGGLE_ATTRIBUTE = "data-tree-toggle";
const LABEL_ATTRIBUTE = "data-tree-label";
const RAIL_ATTRIBUTE = "data-tree-rail";

/**
 * Keyboard focus stays in the search input, which drives the tree as a
 * combobox: the arrow keys move the active row (`aria-activedescendant`) and
 * Enter opens it. Left and Right expand and collapse only while the search is
 * empty; with a query, the tree is fully expanded and they move the caret.
 */
function TreePanel({
  nents,
  selectedPath,
  isNarrow,
  onSelect,
}: {
  nents: Nent[];
  selectedPath: string;
  /** Narrow screens are usually touch screens. */
  isNarrow: boolean;
  onSelect: (nent: Nent) => void;
}) {
  const geometry = isNarrow ? TOUCH : POINTER;
  const roots = useMemo(() => buildNentTree(nents), [nents]);
  // The selected component starts out visible, even when it's nested.
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(
    () => new Set(ancestorPaths(selectedPath)),
  );
  const [query, setQuery] = useState("");
  const rows = useMemo(
    () => visibleNentRows(roots, expanded, query),
    [roots, expanded, query],
  );
  // A search shows every match's ancestors expanded, so expanding and
  // collapsing wait until it's cleared.
  const isSearching = query.trim() !== "";
  const rowCounts = useMemo(() => subtreeRowCounts(rows), [rows]);

  const [activePathState, setActivePath] = useState<string | null>(null);
  // Searching or collapsing can hide the active row. The first match then
  // takes over during a search, and otherwise the selected row, then the
  // first one.
  const isVisible = (path: string | null) =>
    path !== null && rows.some((r) => r.node.nent.path === path);
  const activeIndex = Math.max(
    0,
    rows.findIndex(
      (r) =>
        r.node.nent.path ===
        (isVisible(activePathState)
          ? activePathState
          : query.trim() !== ""
            ? rows.find((m) => m.matchesQuery)?.node.nent.path
            : isVisible(selectedPath)
              ? selectedPath
              : null),
    ),
  );
  const activeRow = rows.at(activeIndex);
  const activePath = activeRow?.node.nent.path;

  const treeId = useId();
  const rowId = (path: string) => `${treeId}-${path}`;
  const rowRefs = useRef(new Map<string, HTMLDivElement>());
  // Scrolls the active row into view when the keyboard or the search moves it,
  // or when the tree opens. A row the mouse hovers is already in view, and
  // scrolling to a half-visible one would move the list under the mouse.
  const scrollToActive = useRef(true);
  useEffect(() => {
    if (activePath !== undefined && scrollToActive.current) {
      rowRefs.current.get(activePath)?.scrollIntoView?.({ block: "nearest" });
    }
    scrollToActive.current = false;
  }, [activePath]);

  const setIsExpanded = (path: string, isExpanded: boolean) =>
    setExpanded((prev) => {
      const next = new Set(prev);
      if (isExpanded) {
        next.add(path);
      } else {
        next.delete(path);
      }
      return next;
    });

  const expandablePaths = useMemo(() => expandableNentPaths(roots), [roots]);
  const isAllExpanded = expandablePaths.every((p) => expanded.has(p));
  // With only top-level components, rows drop the toggle column and start with
  // the name, like the options of `Combobox`.
  const hasToggleColumn = expandablePaths.length > 0;

  const onRowClick = (row: NentTreeRow, e: MouseEvent) => {
    const target = e.target instanceof Element ? e.target : null;
    if (row.hasChildren && target?.closest(`[${TOGGLE_ATTRIBUTE}]`)) {
      setIsExpanded(row.node.nent.path, !row.isExpanded);
    } else if (target?.closest(`[${RAIL_ATTRIBUTE}]`)) {
      // A row's rail runs down its own children.
      setIsExpanded(row.node.nent.path, false);
    } else if (target?.closest(`[${LABEL_ATTRIBUTE}]`)) {
      onSelect(row.node.nent);
    }
  };

  // Returns whether the key was handled, so that the input ignores it.
  const handleKey = (key: string): boolean => {
    if (!activeRow) {
      return false;
    }
    const path = activeRow.node.nent.path;
    scrollToActive.current = true;
    const activate = (i: number) =>
      setActivePath(
        rows[Math.max(0, Math.min(rows.length - 1, i))].node.nent.path,
      );
    switch (key) {
      case "ArrowDown":
        activate(activeIndex + 1);
        return true;
      case "ArrowUp":
        activate(activeIndex - 1);
        return true;
      case "ArrowRight":
        if (isSearching) {
          return false;
        }
        if (activeRow.hasChildren && !activeRow.isExpanded) {
          setIsExpanded(path, true);
        } else if (activeRow.hasChildren) {
          activate(activeIndex + 1);
        }
        return true;
      case "ArrowLeft":
        if (isSearching) {
          return false;
        }
        if (activeRow.hasChildren && activeRow.isExpanded) {
          setIsExpanded(path, false);
        } else if (activeRow.parentPath !== null) {
          setActivePath(activeRow.parentPath);
        }
        return true;
      case "Home":
      case "End":
        if (isSearching) {
          return false;
        }
        activate(key === "Home" ? 0 : rows.length - 1);
        return true;
      case "Enter":
        onSelect(activeRow.node.nent);
        return true;
      default:
        return false;
    }
  };

  return (
    <div
      className={cn(
        "flex flex-col",
        isNarrow ? "max-h-[70dvh] text-sm" : "max-h-80 text-xs",
      )}
    >
      {/* The input spans the whole bar, so all of it focuses the input; the
          icon sits on top and lets clicks through. */}
      <div className="relative border-b">
        {/* Lined up with the top-level rows: the icon over the toggles, and
            the text with the names. Without toggles, the icon lines up with
            the names instead, as in `Combobox`. */}
        <MagnifyingGlassIcon
          style={{
            left: hasToggleColumn
              ? PADDING + geometry.row / 2 - SEARCH_ICON_SIZE / 2
              : PADDING + LABEL_PADDING,
          }}
          className="pointer-events-none absolute top-1/2 -translate-y-1/2 text-content-secondary"
        />
        <input
          // On a touch screen, focusing the input would open the on-screen
          // keyboard over the sheet before the user asked to search.
          autoFocus={!isNarrow}
          type="text"
          role="combobox"
          aria-expanded
          aria-haspopup="tree"
          aria-controls={treeId}
          aria-autocomplete="list"
          aria-activedescendant={
            activePath === undefined ? undefined : rowId(activePath)
          }
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActivePath(null);
            scrollToActive.current = true;
          }}
          onKeyDown={(e) => {
            if (handleKey(e.key)) {
              e.preventDefault();
            }
          }}
          placeholder="Search components..."
          aria-label="Search components"
          style={{
            height: geometry.search,
            paddingLeft: hasToggleColumn
              ? PADDING + geometry.row + LABEL_GAP + LABEL_PADDING
              : PADDING + LABEL_PADDING + SEARCH_ICON_SIZE + SEARCH_ICON_GAP,
            // Clears the expand-all button.
            paddingRight:
              expandablePaths.length > 0
                ? PADDING + geometry.row + LABEL_GAP
                : 12,
          }}
          className={cn(
            "block w-full bg-transparent text-content-primary placeholder:text-content-tertiary focus:outline-hidden",
            // iOS zooms into inputs with text smaller than 16px.
            isNarrow ? "text-base" : "text-sm",
          )}
        />
        {expandablePaths.length > 0 && (
          <ExpandAllButton
            isAllExpanded={isAllExpanded}
            // A search shows every match's ancestors expanded already.
            disabled={isSearching}
            size={geometry.row}
            treeId={treeId}
            onClick={() =>
              setExpanded(isAllExpanded ? new Set() : new Set(expandablePaths))
            }
          />
        )}
      </div>
      <div
        id={treeId}
        role="tree"
        aria-label="Components"
        className={cn("scrollbar overflow-y-auto", rows.length > 0 && "py-1")}
      >
        {rows.map((row, index) => (
          <TreeRow
            key={row.node.nent.path}
            ref={(el) => {
              if (el) {
                rowRefs.current.set(row.node.nent.path, el);
              } else {
                rowRefs.current.delete(row.node.nent.path);
              }
            }}
            id={rowId(row.node.nent.path)}
            row={row}
            geometry={geometry}
            subtreeRowCount={rowCounts[index]}
            isActiveParent={row.node.nent.path === activeRow?.parentPath}
            canToggle={!isSearching}
            isSelected={row.node.nent.path === selectedPath}
            isActive={row.node.nent.path === activePath}
            hasToggleColumn={hasToggleColumn}
            onClick={(e) => onRowClick(row, e)}
            onMouseMove={() => {
              // A key that didn't move the active row leaves the flag set.
              scrollToActive.current = false;
              setActivePath(row.node.nent.path);
            }}
          />
        ))}
      </div>
      {rows.length === 0 && (
        // Outside the tree, which may only contain tree items. Announced as
        // the search changes.
        <div role="status" className="px-3 py-2.5 text-content-secondary">
          No components matching “{query}”.
        </div>
      )}
    </div>
  );
}

/**
 * Sits at the end of the search bar, centered vertically and as far from the
 * edge as the toggles are, so it's in view however far the tree is scrolled.
 */
function ExpandAllButton({
  isAllExpanded,
  disabled,
  size,
  treeId,
  onClick,
}: {
  isAllExpanded: boolean;
  disabled: boolean;
  /** The button is a square as tall as a row. */
  size: number;
  treeId: string;
  onClick: () => void;
}) {
  const label = isAllExpanded ? "Collapse all" : "Expand all";
  return (
    <Tooltip tip={label} side="top" asChild>
      {/* `@ui/Button` doesn't forward `onMouseDown`, which keeps focus in the
          search input. */}
      {/* eslint-disable-next-line react/forbid-elements */}
      <button
        type="button"
        aria-label={label}
        aria-controls={treeId}
        disabled={disabled}
        onMouseDown={(e) => e.preventDefault()}
        onClick={onClick}
        style={{ right: PADDING, width: size, height: size }}
        className={cn(
          "absolute top-1/2 flex -translate-y-1/2 items-center justify-center rounded-md text-content-secondary",
          "hover:bg-border-transparent/70 hover:text-content-primary",
          "disabled:cursor-not-allowed disabled:opacity-50 disabled:hover:bg-transparent disabled:hover:text-content-secondary",
        )}
      >
        {isAllExpanded ? <ChevronsInwardIcon /> : <ChevronsOutwardIcon />}
      </button>
    </Tooltip>
  );
}

// Two chevrons pointing apart (expand) or together (collapse), drawn on the
// 15px grid of the Radix icons elsewhere in the tree.
function ChevronsOutwardIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 15 15" fill="none" aria-hidden>
      <path
        d="M4.5 5.5L7.5 2.5L10.5 5.5M4.5 9.5L7.5 12.5L10.5 9.5"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

function ChevronsInwardIcon() {
  return (
    <svg width="15" height="15" viewBox="0 0 15 15" fill="none" aria-hidden>
      <path
        d="M4.5 2.5L7.5 5.5L10.5 2.5M4.5 12.5L7.5 9.5L10.5 12.5"
        stroke="currentColor"
        strokeLinecap="round"
        strokeLinejoin="round"
      />
    </svg>
  );
}

/** The tree's dimensions, in pixels. */
type Geometry = {
  /** Rows are as tall as the toggle is wide, so the toggle is a square. */
  row: number;
  /**
   * Horizontal space per nesting level. More than half the toggle, so that a
   * child's toggle sits right of the rail coming down from its parent's.
   */
  indent: number;
  /**
   * The rail is 1px wide but can be clicked anywhere this wide, centered on it.
   * At most the space between the rail and the children's toggles.
   */
  railHitWidth: number;
  /** The search bar's height. */
  search: number;
};

const POINTER: Geometry = {
  row: 28,
  indent: 20,
  railHitWidth: 10,
  search: 38,
};
// Narrow screens are usually touch screens, where rows need to be tall enough
// to tap.
const TOUCH: Geometry = {
  row: 40,
  indent: 28,
  railHitWidth: 14,
  search: 44,
};

const PADDING = 4;
// Between a row's toggle and its name's highlight, and inside the highlight
// before the name.
const LABEL_GAP = 2;
const LABEL_PADDING = 6;
// The search icon is 15px wide.
const SEARCH_ICON_SIZE = 15;
// Between the search icon and the text, when the icon lines up with the names.
const SEARCH_ICON_GAP = 8;

function TreeRow({
  ref,
  id,
  row,
  geometry,
  subtreeRowCount,
  isActiveParent,
  canToggle,
  isSelected,
  isActive,
  hasToggleColumn,
  onClick,
  onMouseMove,
}: {
  ref: (el: HTMLDivElement | null) => void;
  id: string;
  row: NentTreeRow;
  geometry: Geometry;
  subtreeRowCount: number;
  isActiveParent: boolean;
  /** Whether the toggle and the rail respond to the mouse. */
  canToggle: boolean;
  isSelected: boolean;
  isActive: boolean;
  /** Without one, the row starts with the name. */
  hasToggleColumn: boolean;
  onClick: (e: MouseEvent) => void;
  onMouseMove: () => void;
}) {
  const { node, level, hasChildren, isExpanded, matchesQuery } = row;
  const { row: rowHeight, indent } = geometry;
  const toggleLeft = PADDING + (level - 1) * indent;

  return (
    // The search input keeps keyboard focus and handles the keys for every row.
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events
    <div
      ref={ref}
      id={id}
      role="treeitem"
      aria-level={level}
      aria-expanded={hasChildren ? isExpanded : undefined}
      aria-selected={isSelected}
      tabIndex={-1}
      onClick={onClick}
      // Keeps focus in the search input.
      onMouseDown={(e) => e.preventDefault()}
      style={{ height: rowHeight, paddingLeft: toggleLeft }}
      className="group/row relative flex items-center pr-1 text-content-primary select-none"
    >
      {subtreeRowCount > 1 && (
        <Rail
          left={toggleLeft + rowHeight / 2}
          geometry={geometry}
          rowCount={subtreeRowCount}
          isActiveParent={isActiveParent}
          isInteractive={canToggle}
        />
      )}
      {hasChildren ? (
        <span
          {...(canToggle && { [TOGGLE_ATTRIBUTE]: true })}
          aria-hidden
          // Moves the active row here, so no other row stays highlighted; this
          // row's name isn't highlighted while its toggle is hovered.
          onMouseMove={canToggle ? onMouseMove : undefined}
          style={{ width: rowHeight, height: rowHeight }}
          className={cn(
            "relative flex shrink-0 items-center justify-center rounded-md text-content-secondary",
            canToggle &&
              "cursor-pointer hover:bg-border-transparent/70 hover:text-content-primary",
          )}
        >
          <ChevronRightIcon
            className={cn("transition-transform", isExpanded && "rotate-90")}
          />
        </span>
      ) : hasToggleColumn ? (
        <span style={{ width: rowHeight }} className="shrink-0" />
      ) : null}
      <span
        {...{ [LABEL_ATTRIBUTE]: true }}
        onMouseMove={onMouseMove}
        style={{
          marginLeft: hasToggleColumn ? LABEL_GAP : 0,
          paddingInline: LABEL_PADDING,
        }}
        className={cn(
          "flex h-full min-w-0 flex-1 cursor-pointer items-center rounded-md",
          isActive && "bg-background-tertiary",
          // Hovering the toggle highlights only the toggle, since a click there
          // expands rather than opens.
          "group-has-[[data-tree-toggle]:hover]/row:bg-transparent",
        )}
      >
        <Tooltip
          tip={
            isUnmounted(node.nent) ? "This component is unmounted" : undefined
          }
          side="right"
          asChild
        >
          <span
            className={cn(
              "truncate",
              isSelected && "font-semibold",
              !matchesQuery && "text-content-tertiary",
            )}
          >
            {node.label}
            {isUnmounted(node.nent) && (
              <>
                <span aria-hidden>*</span>
                <span className="sr-only"> (unmounted)</span>
              </>
            )}
          </span>
        </Tooltip>
      </span>
    </div>
  );
}

// Space between a component's toggle and the top of its rail, and between the
// rail's bottom and the row after the component's subtree.
const RAIL_GAP = 3;

/**
 * A line down the center of an expanded component's chevron, running alongside
 * its children. It starts just below the toggle and ends just above the row
 * after the subtree. Clicking it collapses the component, like the toggle.
 * It's darker while the active row is one of the component's children.
 *
 * It's drawn from the component's own row and spans the rows below, so that it
 * hovers and clicks as one piece. It stacks above those rows, which would
 * otherwise take its clicks.
 */
function Rail({
  left,
  geometry,
  rowCount,
  isActiveParent,
  isInteractive,
}: {
  /** The line's horizontal position in the row, in pixels. */
  left: number;
  geometry: Geometry;
  /** The rows of the component's visible subtree, its own included. */
  rowCount: number;
  isActiveParent: boolean;
  isInteractive: boolean;
}) {
  return (
    <span
      {...(isInteractive && { [RAIL_ATTRIBUTE]: true })}
      aria-hidden
      className={cn(
        "group/rail absolute z-10 flex justify-center",
        isInteractive ? "cursor-pointer" : "pointer-events-none",
      )}
      style={{
        left: left - geometry.railHitWidth / 2 + 0.5,
        width: geometry.railHitWidth,
        top: geometry.row + RAIL_GAP,
        height: (rowCount - 1) * geometry.row - 2 * RAIL_GAP,
      }}
    >
      <span
        className={cn(
          "h-full w-px",
          isInteractive && "group-hover/rail:bg-border-selected",
          isActiveParent ? "bg-border-selected/50" : "bg-border-transparent",
        )}
      />
    </span>
  );
}
