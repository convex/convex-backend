import {
  CSSProperties,
  RefObject,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { NextRouter, useRouter } from "next/router";
import { FileTree, useFileTree } from "@pierre/trees/react";
import type { FileTreeRowDecoration } from "@pierre/trees";
import type { FileOrFolder, ModuleFunction } from "@common/lib/functions/types";
import {
  itemIdentifier,
  useCurrentOpenFunction,
} from "@common/lib/functions/FunctionsProvider";
import { useCurrentGloballyOpenFunction } from "@common/features/functionRunner/lib/functionRunner";
import { DeploymentInfoContext } from "@common/lib/deploymentContext";
import {
  markSearchCursor,
  openFunctionRowsInNewTab,
  preventMultipleSelection,
  SEARCH_CURSOR_ATTRIBUTE,
  SearchCursorMarker,
} from "@common/features/functions/components/functionTreeDom";

type FileTreeModel = ReturnType<typeof useFileTree>["model"];

// `@pierre/trees` models a tree of file paths. Folders and files with several
// functions become directories (`dir/file.js/`), and each function is a leaf
// under its file. A file with a single function is a single leaf labelled
// `file:function`, as the sidebar always showed it.
type FunctionTreePaths = {
  paths: string[];
  functionsByPath: Map<string, ModuleFunction>;
  // Keyed by `itemIdentifier`.
  pathsByFunction: Map<string, string>;
  rootDirectoryPaths: string[];
  directoryPaths: string[];
  // Position of each path in `paths`.
  order: Map<string, number>;
};

// A path segment can't contain `/`, which HTTP action names like
// `GET /api/messages` do, so it's shown as the look-alike U+2215.
function segment(name: string) {
  return name.replaceAll("/", "∕");
}

function functionLabel(f: ModuleFunction, inlineFileName: boolean) {
  return inlineFileName &&
    f.file.name !== f.name &&
    // HTTP actions have unique names.
    f.udfType !== "HttpAction"
    ? `${f.file.name}:${f.name}`
    : f.name;
}

export function toFunctionTreePaths(
  rootEntries: FileOrFolder[],
): FunctionTreePaths {
  const result: FunctionTreePaths = {
    paths: [],
    functionsByPath: new Map(),
    pathsByFunction: new Map(),
    rootDirectoryPaths: [],
    directoryPaths: [],
    order: new Map(),
  };
  const addDirectory = (path: string, depth: number) => {
    result.paths.push(path);
    result.directoryPaths.push(path);
    if (depth === 0) {
      result.rootDirectoryPaths.push(path);
    }
  };
  const addFunction = (path: string, f: ModuleFunction) => {
    result.paths.push(path);
    result.functionsByPath.set(path, f);
    result.pathsByFunction.set(itemIdentifier(f), path);
  };
  // Paths without their trailing `/`: the tree rejects a file and a directory
  // at the same path too.
  const usedPaths = new Set<string>();
  // Several entries can share a name: a `messages.js` file next to a
  // `messages/` folder, or `seed.js` and `seed.dev.js`, which both display as
  // `seed`. Each repeat gets one more U+2060 WORD JOINER, which renders as
  // nothing.
  const uniquePath = (prefix: string, name: string) => {
    let path = `${prefix}${segment(name)}`;
    while (usedPaths.has(path)) {
      path += "\u2060";
    }
    usedPaths.add(path);
    return path;
  };
  const walk = (entries: FileOrFolder[], prefix: string, depth: number) => {
    for (const entry of entries) {
      if (entry.type === "folder") {
        const path = `${uniquePath(prefix, entry.name)}/`;
        addDirectory(path, depth);
        walk(entry.children, path, depth + 1);
      } else if (entry.functions.length === 1) {
        const [f] = entry.functions;
        addFunction(uniquePath(prefix, functionLabel(f, true)), f);
      } else if (entry.functions.length > 1) {
        const path = `${uniquePath(prefix, entry.name)}/`;
        addDirectory(path, depth);
        for (const f of entry.functions) {
          addFunction(uniquePath(path, functionLabel(f, false)), f);
        }
      }
    }
  };
  walk(rootEntries, "", 0);
  result.paths.forEach((path, index) => result.order.set(path, index));
  return result;
}

// Directory paths end with `/` in `paths`, but the tree may hand them to its
// sort comparator without it.
function orderOf({ order }: FunctionTreePaths, path: string) {
  return order.get(path) ?? order.get(`${path}/`) ?? 0;
}

function ancestorDirectories(path: string) {
  const ancestors: string[] = [];
  let index = path.indexOf("/");
  while (index !== -1 && index < path.length - 1) {
    ancestors.push(path.slice(0, index + 1));
    index = path.indexOf("/", index + 1);
  }
  return ancestors;
}

const FUNCTION_ICON_ID = "convex-function";
const CHEVRON_ICON_ID = "radix-chevron-down";

// `FunctionIcon` from `@common/elements/icons` and Radix's `ChevronDownIcon`, as
// sprites the tree can use inside its shadow root.
const SPRITE_SHEET = `<svg xmlns="http://www.w3.org/2000/svg" style="display:none"><symbol id="${FUNCTION_ICON_ID}" viewBox="0 0 24 24" fill="currentColor"><path d="M4.77207 20C4.22502 20 3.79437 19.8619 3.48012 19.5858C3.16004 19.3096 3 18.9759 3 18.5847C3 18.3028 3.09602 18.0755 3.28807 17.9029C3.4743 17.7303 3.68671 17.644 3.92532 17.644C4.1581 17.644 4.3676 17.7217 4.55383 17.877C4.73424 18.0381 4.82444 18.2539 4.82444 18.5243C4.82444 18.6681 4.77789 18.8091 4.68477 18.9471C4.58584 19.091 4.53637 19.1888 4.53637 19.2406C4.53637 19.3038 4.56838 19.3528 4.6324 19.3873C4.69059 19.4218 4.76334 19.4391 4.85063 19.4391C5.29874 19.4391 5.63919 19.0852 5.87197 18.3776C6.10475 17.6757 6.58778 15.6217 7.32105 12.2157L8.05432 9.1521H6.74491L6.91077 8.41855H8.22017C8.32493 7.9813 8.54607 7.38871 8.88361 6.64078C9.22696 5.88709 9.66343 5.28012 10.193 4.81985C10.8332 4.27328 11.4908 4 12.1659 4C12.7071 4 13.1494 4.12082 13.4927 4.36246C13.8419 4.6041 14.0165 4.96368 14.0165 5.44121C14.0165 5.72312 13.9263 5.95038 13.7459 6.12298C13.5713 6.28982 13.3618 6.37325 13.1174 6.37325C12.8788 6.37325 12.6722 6.28982 12.4976 6.12298C12.323 5.95613 12.2357 5.76052 12.2357 5.53614C12.2357 5.3808 12.2823 5.22258 12.3754 5.06149C12.4743 4.89464 12.5238 4.79684 12.5238 4.76807C12.5238 4.7278 12.5034 4.67602 12.4627 4.61273C12.4161 4.5552 12.3346 4.52643 12.2182 4.52643C11.8923 4.52643 11.5955 4.75081 11.3278 5.19957C11.066 5.64833 10.8215 6.30421 10.5946 7.16721L10.2629 8.41855H11.9214L11.7556 9.1521H10.1406C9.29098 13.283 8.58972 15.9928 8.03686 17.2816C7.25703 19.0939 6.16877 20 4.77207 20Z"/><path d="M12.0786 16.3495L13.676 10.7055C13.7401 10.4754 13.7866 10.2941 13.8157 10.1618C13.8739 9.91442 13.903 9.74182 13.903 9.64401C13.903 9.43114 13.8361 9.29881 13.7022 9.24703C13.5684 9.19525 13.3094 9.16361 12.9253 9.1521V8.68608C13.6702 8.64006 14.2493 8.58828 14.6625 8.53074C15.0757 8.47321 15.6925 8.36677 16.5131 8.21143L15.4306 11.4822L15.4743 11.5081C15.4452 11.5369 15.6198 11.2751 15.9981 10.7228C16.3763 10.1647 16.7371 9.71305 17.0805 9.36785C17.418 9.0169 17.7701 8.73786 18.1368 8.53074C18.5092 8.31787 18.8991 8.21143 19.3065 8.21143C19.6848 8.21143 20.0049 8.31787 20.2667 8.53074C20.5344 8.74362 20.6683 9.09169 20.6683 9.57497C20.6683 10.0237 20.4064 11.0133 19.8826 12.5437C19.3531 14.0683 19.0883 14.8853 19.0883 14.9946C19.0883 15.0982 19.1203 15.1787 19.1843 15.2362C19.2483 15.288 19.3123 15.3139 19.3763 15.3139C19.5335 15.3139 19.7662 15.1183 20.0747 14.7271C20.2376 14.52 20.4151 14.2639 20.6072 13.959L21 14.2093C20.5577 14.9169 20.1823 15.4261 19.8739 15.7368C19.3385 16.2718 18.7711 16.5394 18.1717 16.5394C17.8516 16.5394 17.5635 16.4473 17.3075 16.2632C17.0572 16.0791 16.9321 15.7886 16.9321 15.3916C16.9321 15.1384 16.9932 14.7702 17.1154 14.2869C17.2376 13.8037 17.386 13.306 17.5606 12.794L18.451 10.1532C18.4685 10.0899 18.483 10.0352 18.4947 9.98921C18.5063 9.94319 18.5121 9.89716 18.5121 9.85113C18.5121 9.71881 18.4656 9.62675 18.3725 9.57497C18.2852 9.52319 18.1891 9.4973 18.0844 9.4973C17.7177 9.4973 17.3104 9.77922 16.8623 10.343C16.4142 10.9011 16.0475 11.4592 15.7624 12.0173C15.4539 12.6214 15.195 13.2341 14.9855 13.8554C14.7759 14.4768 14.5257 15.3082 14.2347 16.3495H12.0786Z"/></symbol><symbol id="${CHEVRON_ICON_ID}" viewBox="0 0 15 15"><path fill="currentColor" fill-rule="evenodd" clip-rule="evenodd" d="M3.13523 6.15803C3.3241 5.95657 3.64052 5.94637 3.84197 6.13523L7.5 9.56464L11.158 6.13523C11.3595 5.94637 11.6759 5.95657 11.8648 6.15803C12.0536 6.35949 12.0434 6.67591 11.842 6.86477L7.84197 10.6148C7.64964 10.7951 7.35036 10.7951 7.15803 10.6148L3.15803 6.86477C2.95657 6.67591 2.94637 6.35949 3.13523 6.15803Z"/></symbol></svg>`;

// Styled as a pill by `UNSAFE_CSS`, which also shortens it to "Int." when the
// function name leaves too little room.
const INTERNAL_FUNCTION_DECORATION: FileTreeRowDecoration = {
  text: "Internal",
  title: "Internal function: callable only from other functions",
};

// Tailwind classes don't reach into the tree's shadow root, so the sidebar's
// row styles (`sidebarLinkClassNames` with `font: "mono", small: true`, and
// the accent hover and selection of the previous `DirectoryItem`) are
// restated here against the design system's color tokens.
const TREE_STYLE = {
  "--trees-bg-override": "var(--color-background-secondary)",
  "--trees-fg-override": "var(--color-content-primary)",
  "--trees-fg-muted-override": "var(--color-content-tertiary)",
  "--trees-accent-override": "var(--color-util-accent)",
  "--trees-font-family-override": "var(--font-mono)",
  "--trees-font-size-override": "12px",
  "--trees-border-radius-override": "0",
  "--trees-item-margin-x-override": "0",
  "--trees-indent-guide-bg-override": "var(--color-border-transparent)",
  // The tree also paints this behind a truncated name's "…" marker, so
  // hovering must go through it rather than a `background-color` rule.
  "--trees-bg-muted-override":
    "color-mix(in srgb, var(--color-util-accent) 20%, transparent)",
  "--trees-selected-bg-override":
    "color-mix(in srgb, var(--color-util-accent) 30%, transparent)",
  "--trees-selected-fg-override": "var(--color-content-primary)",
  "--trees-selected-focused-border-color-override":
    "color-mix(in srgb, var(--color-util-accent) 40%, transparent)",
  "--trees-focus-ring-color-override": "var(--color-util-accent)",
  "--trees-scrollbar-thumb-override": "var(--color-macosScrollbar-thumb)",
  height: "100%",
} as CSSProperties;

// Selection (the open function) fills the row; focus (keyboard navigation in
// the tree, or the search cursor) outlines it.
const UNSAFE_CSS = `
  button[${SEARCH_CURSOR_ATTRIBUTE}]::before {
    content: "";
    position: absolute;
    inset: 0;
    outline: var(--trees-focus-ring-width) solid var(--trees-focus-ring-color);
    outline-offset: calc(-1 * var(--trees-focus-ring-width));
    pointer-events: none;
  }
  [data-item-type='file'] svg {
    color: var(--color-content-tertiary);
  }

  /* Only internal functions have a decoration. */
  [data-item-section='decoration'] {
    container-type: inline-size;
  }
  [data-item-section='decoration']:has(> span) {
    /* Room for "Int.", taken from a long name before it truncates. */
    min-width: 32px;
  }
  [data-item-section='decoration'] > span {
    font-family: var(--font-display);
    font-size: 10px;
    line-height: 16px;
    padding: 0 5px;
    border-radius: 4px;
    background: var(--color-background-tertiary);
    color: var(--color-content-secondary);
  }
  /* "Internal" needs about 50px. */
  @container (max-width: 52px) {
    [data-item-section='decoration'] > span {
      font-size: 0;
    }
    [data-item-section='decoration'] > span::before {
      content: "Int.";
      font-size: 10px;
    }
  }
`;

export type FunctionTreeProps = {
  tree: FileOrFolder[];
  searchTerm: string;
  onChangeFunction: () => void;
  /**
   * Contains the search input. While it has focus, the arrow keys move a
   * cursor through the tree's rows and Enter opens the function under it.
   */
  searchBox?: RefObject<HTMLElement | null>;
};

export function FunctionTree({
  tree,
  searchTerm,
  onChangeFunction,
  searchBox,
}: FunctionTreeProps) {
  const router = useRouter();
  const currentOpenFunction = useCurrentOpenFunction();
  const [, setGloballyOpenFunction] = useCurrentGloballyOpenFunction();
  const { useLogDeploymentEvent } = useContext(DeploymentInfoContext);
  const log = useLogDeploymentEvent();

  const treePaths = useMemo(() => toFunctionTreePaths(tree), [tree]);
  const currentPath = currentOpenFunction
    ? treePaths.pathsByFunction.get(itemIdentifier(currentOpenFunction))
    : undefined;

  // The row that Enter opens while the search input has focus.
  const [cursor, setCursor] = useState<string | null>(null);
  const [searchBoxFocused, setSearchBoxFocused] = useState(false);

  // The model is created once, so its callbacks read the latest render's
  // values through this ref.
  const latest = useRef({
    treePaths,
    currentPath,
    cursor: searchBoxFocused ? cursor : null,
    hrefFor: (_path: string): string | undefined => undefined,
    openFunction: (_f: ModuleFunction) => {},
  });
  latest.current = {
    treePaths,
    currentPath,
    cursor: searchBoxFocused ? cursor : null,
    hrefFor: (path) => {
      const f = treePaths.functionsByPath.get(path);
      return f && functionHref(router, f);
    },
    openFunction: (f) => {
      onChangeFunction();
      setGloballyOpenFunction(f);
      log("change function");
      void router.push({ query: { ...router.query, function: f.displayName } });
    },
  };

  const { model } = useFileTree({
    paths: treePaths.paths,
    // Keeps the sidebar's order (`generateFileTree`'s), which differs from the
    // tree's default of folders first.
    sort: (left, right) =>
      orderOf(latest.current.treePaths, left.path) -
      orderOf(latest.current.treePaths, right.path),
    initialExpandedPaths: initialExpandedPaths(
      treePaths,
      searchTerm,
      currentPath,
    ),
    initialSelectedPaths: currentPath ? [currentPath] : [],
    itemHeight: 30,
    icons: {
      set: "none",
      spriteSheet: SPRITE_SHEET,
      remap: {
        "file-tree-icon-file": { name: FUNCTION_ICON_ID, viewBox: "0 0 24 24" },
        // At its native 15px, as in the component switcher, rather than
        // stretched to the tree's 16px icon slot.
        "file-tree-icon-chevron": {
          name: CHEVRON_ICON_ID,
          viewBox: "0 0 15 15",
          width: 15,
          height: 15,
        },
      },
    },
    renderRowDecoration: ({ item }) => {
      const f = latest.current.treePaths.functionsByPath.get(item.path);
      return f && f.visibility.kind !== "public"
        ? INTERNAL_FUNCTION_DECORATION
        : null;
    },
    // TODO: Render function rows as links (Cmd/Ctrl-click, middle-click, "Copy
    // link") once `@pierre/trees` can render a row as an `<a href>`. Until
    // then, they are buttons that open the function through the selection,
    // and `openFunctionRowsInNewTab` handles Cmd/Ctrl-click and middle click.
    //
    // Rows are selected by a click, by Enter or Space on a focused row, or by
    // the effect below that follows the open function.
    onSelectionChange: (selectedPaths) => {
      const {
        treePaths: paths,
        currentPath: current,
        openFunction,
      } = latest.current;
      const path = selectedPaths.at(-1);
      if (path === undefined) {
        return;
      }
      const selected = paths.functionsByPath.get(path);
      if (selected) {
        if (path !== current) {
          openFunction(selected);
        }
        return;
      }
      // A directory: the click toggles it, and the selection goes back to the
      // open function once the tree is done handling it.
      queueMicrotask(() => {
        model.getItem(path)?.deselect();
        const { currentPath: open } = latest.current;
        if (open) {
          model.getItem(open)?.select();
        }
      });
    },
    unsafeCSS: UNSAFE_CSS,
  });

  // The search cursor, new tabs, and no selection of several rows.
  const cursorMarker = useRef<SearchCursorMarker | null>(null);
  const hasRows = tree.length > 0;
  useEffect(() => {
    if (!hasRows) {
      return;
    }
    let detach = () => {};
    // `FileTree` renders into its element a commit after it mounts.
    let frame = requestAnimationFrame(function attach() {
      const shadowRoot = model.getFileTreeContainer()?.shadowRoot;
      if (!shadowRoot) {
        frame = requestAnimationFrame(attach);
        return;
      }
      const marker = markSearchCursor(shadowRoot, () => latest.current.cursor);
      const stopNewTabs = openFunctionRowsInNewTab(shadowRoot, (path) =>
        latest.current.hrefFor(path),
      );
      const allowMultipleSelection = preventMultipleSelection(shadowRoot);
      cursorMarker.current = marker;
      detach = () => {
        marker.detach();
        stopNewTabs();
        allowMultipleSelection();
      };
    });
    return () => {
      cancelAnimationFrame(frame);
      detach();
      cursorMarker.current = null;
    };
  }, [model, hasRows]);
  useEffect(() => {
    cursorMarker.current?.sync();
  }, [cursor, searchBoxFocused]);

  // The model holds expansion state. A new list of paths, as on every push,
  // keeps what the user expanded. A search expands every directory, and
  // clearing it goes back to what was expanded before.
  const previous = useRef({ treePaths, searchTerm });
  const expandedBeforeSearch = useRef<string[]>([]);
  useEffect(() => {
    const { treePaths: previousPaths, searchTerm: previousSearchTerm } =
      previous.current;
    previous.current = { treePaths, searchTerm };
    if (previousPaths === treePaths) {
      // The first render, which `useFileTree` already expanded for.
      setCursor(searchTerm ? firstFunctionRow(model, treePaths) : null);
      return;
    }
    const expanded = previousPaths.directoryPaths.filter((path) => {
      const item = model.getItem(path);
      return item !== null && "isExpanded" in item && item.isExpanded();
    });
    let expandedPaths: string[];
    if (searchTerm && !previousSearchTerm) {
      expandedBeforeSearch.current =
        previousPaths.paths.length === 0
          ? initialExpandedPaths(treePaths, "", latest.current.currentPath)
          : expanded;
      expandedPaths = treePaths.directoryPaths;
    } else if (searchTerm) {
      expandedPaths = treePaths.directoryPaths;
    } else if (previousSearchTerm) {
      // Also after a search that matched nothing, which emptied the paths.
      expandedPaths = [
        ...expandedBeforeSearch.current,
        ...(latest.current.currentPath
          ? ancestorDirectories(latest.current.currentPath)
          : []),
      ];
    } else if (previousPaths.paths.length === 0) {
      // The first functions to arrive.
      expandedPaths = initialExpandedPaths(
        treePaths,
        searchTerm,
        latest.current.currentPath,
      );
    } else {
      expandedPaths = expanded;
    }
    model.resetPaths(treePaths.paths, { initialExpandedPaths: expandedPaths });
    setCursor(searchTerm ? firstFunctionRow(model, treePaths) : null);
  }, [model, treePaths, searchTerm]);

  // Keeps the selection on the open function, however it was opened (a click
  // here, a link elsewhere, or the URL).
  useEffect(() => {
    for (const path of model.getSelectedPaths()) {
      if (path !== currentPath) {
        model.getItem(path)?.deselect();
      }
    }
    if (currentPath && !model.getItem(currentPath)?.isSelected()) {
      model.getItem(currentPath)?.select();
    }
  }, [model, currentPath, treePaths]);
  // Only when another function opens, so that a push doesn't reopen the
  // directories of the open function or scroll back to it.
  useEffect(() => {
    if (!currentPath) {
      return;
    }
    for (const ancestor of ancestorDirectories(currentPath)) {
      const item = model.getItem(ancestor);
      if (item !== null && "expand" in item) {
        item.expand();
      }
    }
    model.scrollToPath(currentPath, { focus: false, offset: "nearest" });
  }, [model, currentPath]);

  // The arrow keys and Enter in the search input.
  useEffect(() => {
    const element = searchBox?.current;
    if (!element) {
      return;
    }
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "ArrowDown" || event.key === "ArrowUp") {
        event.preventDefault();
        setCursor((current) => {
          const next = adjacentRow(
            model,
            current,
            event.key === "ArrowDown" ? 1 : -1,
          );
          if (next) {
            model.scrollToPath(next, { focus: false, offset: "nearest" });
          }
          return next ?? current;
        });
      } else if (event.key === "Enter") {
        const path = latest.current.cursor;
        const item = path ? model.getItem(path) : null;
        if (!path || !item) {
          return;
        }
        event.preventDefault();
        const f = latest.current.treePaths.functionsByPath.get(path);
        if (f) {
          latest.current.openFunction(f);
        } else if ("toggle" in item) {
          item.toggle();
        }
      }
    };
    const onFocusIn = () => setSearchBoxFocused(true);
    const onFocusOut = () => setSearchBoxFocused(false);
    element.addEventListener("keydown", onKeyDown);
    element.addEventListener("focusin", onFocusIn);
    element.addEventListener("focusout", onFocusOut);
    setSearchBoxFocused(element.contains(document.activeElement));
    return () => {
      element.removeEventListener("keydown", onKeyDown);
      element.removeEventListener("focusin", onFocusIn);
      element.removeEventListener("focusout", onFocusOut);
    };
  }, [model, searchBox]);

  if (tree.length === 0) {
    return (
      <span className="px-3 text-xs text-content-secondary">
        No functions match your search.
      </span>
    );
  }
  return <FileTree model={model} style={TREE_STYLE} />;
}

// The URL of the functions page with `f` open, as `FunctionItem` links to it.
function functionHref(router: NextRouter, f: ModuleFunction) {
  const [pathname, search = ""] = router.asPath.split("#")[0].split("?");
  const query = new URLSearchParams(search);
  query.set("function", f.displayName);
  return `${router.basePath}${pathname}?${query}`;
}

function visibleRowPaths(model: FileTreeModel) {
  const count = model.getVisibleCount();
  return count === 0
    ? []
    : model.getVisibleRows(0, count - 1).map((row) => row.path);
}

function firstFunctionRow(model: FileTreeModel, treePaths: FunctionTreePaths) {
  return (
    visibleRowPaths(model).find((path) =>
      treePaths.functionsByPath.has(path),
    ) ?? null
  );
}

// The visible row `delta` rows from `path`, or the first row when there's no
// `path` or it isn't visible anymore.
function adjacentRow(
  model: FileTreeModel,
  path: string | null,
  delta: number,
): string | null {
  const rows = visibleRowPaths(model);
  const index = path === null ? -1 : rows.indexOf(path);
  if (index === -1) {
    return rows[0] ?? null;
  }
  return rows[index + delta] ?? null;
}

// Root-level entries start open, like the sidebar always opened them; with a
// search, every directory opens so that all matches show.
function initialExpandedPaths(
  treePaths: FunctionTreePaths,
  searchTerm: string,
  currentPath: string | undefined,
) {
  return [
    ...(searchTerm ? treePaths.directoryPaths : treePaths.rootDirectoryPaths),
    ...(currentPath ? ancestorDirectories(currentPath) : []),
  ];
}
