import chalk from "chalk";
import { hierarchy, treemap, treemapSquarify } from "d3-hierarchy";
import stringWidth from "string-width";
import { TreeNode } from "./tree.js";
import {
  ORIGIN_COLORS,
  EXTERNAL_COLOR,
  formatBytes,
  formatPercent,
  ownerColor,
  colorFor,
  truncate,
} from "./format.js";

export type Rect = { x: number; y: number; w: number; h: number };

/**
 * Squarified layout of `values` into `rect`, returned in the same order.
 * Values should be sorted descending.
 */
export function layout(values: number[], rect: Rect): Rect[] {
  type Datum = { value: number; children?: Datum[] };
  const root = hierarchy<Datum>({
    value: 0,
    children: values.map((value) => ({ value })),
  }).sum((d) => (d.children ? 0 : Math.max(0, d.value)));
  const laidOut = treemap<Datum>()
    .tile(treemapSquarify)
    .size([rect.w, rect.h])
    .round(false)(root);
  return (laidOut.children ?? []).map((c) => ({
    x: rect.x + c.x0,
    y: rect.y + c.y0,
    w: c.x1 - c.x0,
    h: c.y1 - c.y0,
  }));
}

/**
 * Color for a node. Packages and origins get their own color, and files and
 * directories inherit the color of the package they're in.
 */
export function nodeColor(node: TreeNode, parentColor?: string): string {
  if (node.kind === "external") return EXTERNAL_COLOR;
  if (node.kind === "overhead") return ORIGIN_COLORS.virtual;
  if (node.kind === "glue") return ORIGIN_COLORS.virtual;
  if (node.kind === "owner" && node.origin) {
    return ownerColor(node.origin, node.name);
  }
  // Only builds grouped under a package have an origin.
  const insidePackage =
    node.kind === "file" ||
    node.kind === "dir" ||
    (node.kind === "build" && node.origin !== undefined);
  if (parentColor && insidePackage) return parentColor;
  return colorFor(node.name);
}

/** Mixes `hex` toward white (positive `amount`) or black (negative), 0 to 1. */
export function shade(hex: string, amount: number): string {
  const n = parseInt(hex.slice(1), 16);
  const target = amount < 0 ? 0 : 255;
  const channel = (shift: number) => {
    const c = (n >> shift) & 0xff;
    return Math.round(c + (target - c) * Math.abs(amount));
  };
  const rgb = (channel(16) << 16) | (channel(8) << 8) | channel(0);
  return `#${rgb.toString(16).padStart(6, "0")}`;
}

/** Near-black or near-white, whichever reads better on `bg`. */
export function contrastText(bg: string): string {
  const n = parseInt(bg.slice(1), 16);
  const luma =
    0.299 * ((n >> 16) & 0xff) + 0.587 * ((n >> 8) & 0xff) + 0.114 * (n & 0xff);
  return luma > 140 ? "#111111" : "#f5f5f5";
}

export type TreemapOptions = {
  width: number;
  height: number;
  /** Levels below the root to draw; deeper levels only nest when there's room. */
  depth: number;
  /** Index of a child of the root to highlight; its siblings are dimmed. */
  highlight?: number | undefined;
  /** Color of the root, inherited by file and directory children. */
  color?: string | undefined;
};

export const TREEMAP_BACKGROUND = "#16181c";

/**
 * Terminal cells are about twice as tall as they are wide, so rects are laid
 * out in a space stretched vertically by this factor to look square.
 */
const CELL_ASPECT = 2;

/** Labels are placed a grapheme cluster per cell, so emoji sequences and combining marks stay whole. */
const graphemes = new Intl.Segmenter(undefined, { granularity: "grapheme" });

const MIN_NEST_WIDTH = 8;
const MIN_NEST_HEIGHT = 4;

type Cell = { ch: string; bg: string; fg: string; bold: boolean };

/** Draws the children of `root` as a treemap, one string per terminal row. */
export function renderTreemap(
  root: TreeNode,
  options: TreemapOptions,
): string[] {
  const { width, height } = options;
  const grid: Cell[][] = Array.from({ length: height }, () =>
    Array.from({ length: width }, () => ({
      ch: " ",
      bg: TREEMAP_BACKGROUND,
      fg: "#ffffff",
      bold: false,
    })),
  );
  drawChildren(
    grid,
    root.children,
    { x: 0, y: 0, w: width, h: height },
    {
      depthLeft: options.depth,
      highlight: options.highlight,
      parentColor: options.color,
      level: 0,
      dimmed: false,
    },
  );
  return grid.map(serializeRow);
}

type DrawState = {
  depthLeft: number;
  highlight: number | undefined;
  parentColor: string | undefined;
  level: number;
  dimmed: boolean;
};

function drawChildren(
  grid: Cell[][],
  children: TreeNode[],
  area: Rect,
  state: DrawState,
) {
  if (area.w < 1 || area.h < 1) return;
  const rects = layout(
    children.map((c) => c.bytes),
    { x: area.x, y: area.y * CELL_ASPECT, w: area.w, h: area.h * CELL_ASPECT },
  );
  const parentBytes = children.reduce((n, c) => n + c.bytes, 0);
  children.forEach((child, idx) => {
    const r = rects[idx];
    // Rounding shared edges the same way keeps neighbors flush, and rects
    // thinner than half a cell vanish instead of overlapping.
    const x0 = Math.round(r.x);
    const x1 = Math.round(r.x + r.w);
    const y0 = Math.round(r.y / CELL_ASPECT);
    const y1 = Math.round((r.y + r.h) / CELL_ASPECT);
    const w = x1 - x0;
    const h = y1 - y0;
    if (w <= 0 || h <= 0) return;

    const base = nodeColor(child, state.parentColor);
    // Siblings that share a color (files in one package) alternate in
    // lightness so their edges stay visible.
    let color =
      state.level === 0 ? base : shade(base, idx % 2 === 0 ? 0 : -0.14);
    const highlighted = state.level === 0 && state.highlight === idx;
    const dimmed =
      state.dimmed ||
      (state.level === 0 && state.highlight !== undefined && !highlighted);
    if (dimmed) color = shade(color, -0.55);

    const nest =
      state.depthLeft > 1 &&
      child.children.length > 0 &&
      w >= MIN_NEST_WIDTH &&
      h >= MIN_NEST_HEIGHT;
    if (nest) {
      const frame = shade(color, -0.35);
      fill(grid, { x: x0, y: y0, w, h }, frame);
      label(grid, x0, y0, w, `${child.name} ${formatBytes(child.bytes)}`, {
        bg: frame,
        bold: true,
      });
      drawChildren(
        grid,
        child.children,
        { x: x0 + 1, y: y0 + 1, w: w - 1, h: h - 1 },
        {
          depthLeft: state.depthLeft - 1,
          highlight: undefined,
          parentColor: base,
          level: state.level + 1,
          dimmed,
        },
      );
      return;
    }

    fill(grid, { x: x0, y: y0, w, h }, color);
    if (w >= 3) {
      const edge = shade(color, -0.45);
      for (let y = y0; y < y1; y++) {
        const cell = grid[y]?.[x0];
        if (cell) {
          cell.ch = "▏";
          cell.fg = edge;
        }
      }
    }
    const textX = w >= 3 ? x0 + 1 : x0;
    const textW = x1 - textX;
    label(grid, textX, y0, textW, child.name, { bg: color, bold: highlighted });
    if (h >= 2) {
      const share = parentBytes > 0 ? child.bytes / parentBytes : 0;
      label(
        grid,
        textX,
        y0 + 1,
        textW,
        `${formatBytes(child.bytes)} ${formatPercent(share)}`,
        { bg: color, bold: false },
      );
    }
  });
}

function fill(grid: Cell[][], r: Rect, bg: string) {
  for (let y = r.y; y < r.y + r.h; y++) {
    for (let x = r.x; x < r.x + r.w; x++) {
      const cell = grid[y]?.[x];
      if (cell) {
        cell.bg = bg;
        cell.ch = " ";
        cell.bold = false;
      }
    }
  }
}

/**
 * Writes `text` into the row starting at column `x`, at most `w` columns wide.
 * Labels narrower than four columns are skipped since they'd show little more
 * than an ellipsis.
 */
function label(
  grid: Cell[][],
  x: number,
  y: number,
  w: number,
  text: string,
  style: { bg: string; bold: boolean },
) {
  if (w < 4) return;
  const row = grid[y];
  if (!row) return;
  const fg = contrastText(style.bg);
  let col = x;
  for (const { segment: ch } of graphemes.segment(truncate(text, w))) {
    const cw = stringWidth(ch);
    if (cw === 0) continue;
    if (col + cw > x + w) break;
    const cell = row[col];
    if (!cell) break;
    cell.ch = ch;
    cell.fg = fg;
    cell.bold = style.bold;
    // A wide character covers the next cell, so that cell prints nothing.
    for (let k = 1; k < cw; k++) {
      const covered = row[col + k];
      if (covered) {
        covered.ch = "";
        covered.bg = style.bg;
      }
    }
    col += cw;
  }
}

function serializeRow(row: Cell[]): string {
  let out = "";
  let run = "";
  let style: Cell | null = null;
  const flush = () => {
    if (!style || run === "") return;
    let s = chalk.bgHex(style.bg).hex(style.fg);
    if (style.bold) s = s.bold;
    out += s(run);
    run = "";
  };
  for (const cell of row) {
    if (
      !style ||
      cell.bg !== style.bg ||
      cell.fg !== style.fg ||
      cell.bold !== style.bold
    ) {
      flush();
      style = cell;
    }
    run += cell.ch;
  }
  flush();
  return out;
}
