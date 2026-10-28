import fuzzy from "fuzzy";
import { NENT_APP_PLACEHOLDER, Nent } from "@common/lib/useNents";

export type NentTreeNode = {
  nent: Nent;
  /** The last segment of the path; the full path is implied by the tree. */
  label: string;
  children: NentTreeNode[];
};

export type NentTreeRow = {
  node: NentTreeNode;
  /** 1-based, as `aria-level` expects. */
  level: number;
  parentPath: string | null;
  /** Children shown under this row, which a search can narrow down. */
  hasChildren: boolean;
  isExpanded: boolean;
  /** False for an ancestor kept only to place a matching descendant. */
  matchesQuery: boolean;
};

function parentPathOf(path: string): string | null {
  const slash = path.lastIndexOf("/");
  return slash === -1 ? null : path.slice(0, slash);
}

/**
 * Nests components under their parent path. The app is a root next to the
 * top-level components rather than their parent, matching how the rest of the
 * dashboard presents it. A component whose parent isn't in the list becomes a
 * root labeled with its full path, so it can't be mistaken for a top-level one.
 * Among siblings, the app comes first and mounted components come before
 * unmounted ones, each group sorted by label.
 */
export function buildNentTree(nents: Nent[]): NentTreeNode[] {
  const byPath = new Map<string, NentTreeNode>();
  for (const nent of nents) {
    byPath.set(nent.path, { nent, label: nent.path, children: [] });
  }

  const roots: NentTreeNode[] = [];
  for (const node of byPath.values()) {
    const { path } = node.nent;
    if (path === NENT_APP_PLACEHOLDER) {
      node.label = "app";
      roots.push(node);
      continue;
    }
    const parentPath = parentPathOf(path);
    const parent = parentPath === null ? undefined : byPath.get(parentPath);
    if (parent) {
      node.label = path.slice(parentPath!.length + 1);
      parent.children.push(node);
    } else {
      roots.push(node);
    }
  }

  const isApp = (node: NentTreeNode) => node.nent.path === NENT_APP_PLACEHOLDER;
  const sortSiblings = (nodes: NentTreeNode[]) => {
    nodes.sort(
      (a, b) =>
        Number(isApp(b)) - Number(isApp(a)) ||
        Number(isUnmounted(a.nent)) - Number(isUnmounted(b.nent)) ||
        a.label.localeCompare(b.label),
    );
    nodes.forEach((n) => sortSiblings(n.children));
  };
  sortSiblings(roots);
  return roots;
}

export function isUnmounted(nent: Nent) {
  return nent.state !== undefined && nent.state !== "active";
}

function searchText(node: NentTreeNode) {
  return node.nent.path === NENT_APP_PLACEHOLDER ? "app" : node.nent.path;
}

/** Paths of every ancestor of `path`, so that it starts out visible. */
export function ancestorPaths(path: string): string[] {
  const result: string[] = [];
  for (let p = parentPathOf(path); p !== null; p = parentPathOf(p)) {
    result.push(p);
  }
  return result;
}

/** Paths of every component with children, which "Expand all" expands. */
export function expandableNentPaths(roots: NentTreeNode[]): string[] {
  const paths: string[] = [];
  const visit = (node: NentTreeNode) => {
    if (node.children.length > 0) {
      paths.push(node.nent.path);
      node.children.forEach(visit);
    }
  };
  roots.forEach(visit);
  return paths;
}

/**
 * Flattens the tree into the rows currently on screen. With a query, only
 * components whose path matches stay, along with their ancestors, which are
 * force-expanded so every match is visible.
 */
export function visibleNentRows(
  roots: NentTreeNode[],
  expanded: ReadonlySet<string>,
  query: string,
): NentTreeRow[] {
  const rows: NentTreeRow[] = [];
  const searching = query.trim() !== "";

  // Returns whether `node` or one of its descendants matches, appending its
  // rows when it does.
  const visit = (
    node: NentTreeNode,
    level: number,
    parentPath: string | null,
  ): boolean => {
    const matchesQuery =
      !searching || fuzzy.test(query.trim(), searchText(node));
    const row: NentTreeRow = {
      node,
      level,
      parentPath,
      hasChildren: node.children.length > 0,
      isExpanded: searching || expanded.has(node.nent.path),
      matchesQuery,
    };
    const index = rows.length;
    rows.push(row);

    let anyChildMatches = false;
    if (row.isExpanded) {
      for (const child of node.children) {
        anyChildMatches =
          visit(child, level + 1, node.nent.path) || anyChildMatches;
      }
    }
    if (searching) {
      row.hasChildren = anyChildMatches;
      row.isExpanded = anyChildMatches;
      if (!matchesQuery && !anyChildMatches) {
        rows.splice(index, 1);
        return false;
      }
    }
    return true;
  };

  for (const root of roots) {
    visit(root, 1, null);
  }
  return rows;
}

/**
 * For each row, how many rows its visible subtree spans, its own included: 1
 * for a leaf or a collapsed component.
 *
 * `rows` must be in the order `visibleNentRows` returns them, a preorder
 * traversal where each subtree's rows follow its root's without a gap: a
 * subtree ends at the next row that is no deeper than its root.
 */
export function subtreeRowCounts(rows: NentTreeRow[]): number[] {
  return rows.map(({ level }, start) => {
    let end = start;
    while (end + 1 < rows.length && rows[end + 1].level > level) {
      end++;
    }
    return end - start + 1;
  });
}
