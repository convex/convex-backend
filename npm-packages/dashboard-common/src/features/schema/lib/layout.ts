export type NodePosition = { x: number; y: number };
export type NodePositions = Record<string, NodePosition>;

// The grid, edge strokes, and arrowheads snap to discrete zoom "steps" (0 at
// zoom >= 1, then +1 per halving) rather than scaling continuously — so they
// stay legible when zoomed out instead of smearing. Returns that step for a
// given zoom; callers scale by `2 ** step`.
export function zoomStep(zoom: number): number {
  return Math.max(0, Math.ceil(Math.log2(1 / Math.max(zoom, 0.0001))));
}

export type NodeSize = { width: number; height: number };

export const NODE_WIDTH = 300;
export const NODE_HEADER_HEIGHT = 36;
export const NODE_ROW_HEIGHT = 24;
const NODE_VERTICAL_PADDING = 8;
// Cap the rendered height so very wide tables don't dominate the canvas.
export const MAX_VISIBLE_ROWS = 24;
// Cap the number of indexes rendered before collapsing into a "+N more" row.
export const MAX_VISIBLE_INDEXES = 5;

// Number of rows a list renders given its length and visible cap: the visible
// items plus one "+N more" row when the list overflows.
function visibleRows(count: number, max: number): number {
  return Math.min(count, max) + (count > max ? 1 : 0);
}

/**
 * Estimate the rendered size of a table node from its field and index counts.
 * The index section adds its own header row plus a row per (visible) index.
 */
export function nodeSize(fieldCount: number, indexCount = 0): NodeSize {
  // A table with no fields still renders a single "no fields" placeholder row.
  const fieldRows =
    fieldCount === 0 ? 1 : visibleRows(fieldCount, MAX_VISIBLE_ROWS);
  const indexRows =
    indexCount > 0 ? visibleRows(indexCount, MAX_VISIBLE_INDEXES) + 1 : 0;
  return {
    width: NODE_WIDTH,
    height:
      NODE_HEADER_HEIGHT +
      (fieldRows + indexRows) * NODE_ROW_HEIGHT +
      NODE_VERTICAL_PADDING * 2,
  };
}

/**
 * Combine a manually-saved layout with a freshly-computed one so that an
 * existing custom arrangement survives schema changes:
 *
 * - When there's no saved layout, the computed layout is used as-is.
 * - Tables with a saved position keep it exactly (so adding or removing a table
 *   never reshuffles the arrangement the user built).
 * - Removed tables simply fall out (we only position tables in `computed`).
 * - Newly-added tables that have no saved position are parked in a column just
 *   to the right of the existing layout, stacked top-down, rather than dropped
 *   wherever the force simulation happened to put them (which could overlap).
 */
export function mergeSavedLayout(
  saved: NodePositions,
  computed: NodePositions,
  sizes: Record<string, NodeSize>,
): NodePositions {
  const tables = Object.keys(computed);
  const savedTables = tables.filter((table) => saved[table]);

  // No custom layout yet — use the freshly computed one.
  if (savedTables.length === 0) {
    return { ...computed };
  }

  const result: NodePositions = {};
  savedTables.forEach((table) => {
    result[table] = saved[table];
  });

  const newTables = tables.filter((table) => !saved[table]);
  if (newTables.length === 0) {
    return result;
  }

  // Bounding box of the kept (saved) nodes.
  let minY = Infinity;
  let maxX = -Infinity;
  savedTables.forEach((table) => {
    const size = sizes[table] ?? { width: NODE_WIDTH, height: 100 };
    minY = Math.min(minY, result[table].y);
    maxX = Math.max(maxX, result[table].x + size.width);
  });

  const GAP = 60;
  const columnX = maxX + GAP;
  let y = minY;
  newTables.forEach((table) => {
    const size = sizes[table] ?? { width: NODE_WIDTH, height: 100 };
    result[table] = { x: columnX, y };
    y += size.height + GAP;
  });
  return result;
}
