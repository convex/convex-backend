import chalk from "chalk";
import cliTruncate from "cli-truncate";
import stringWidth from "string-width";
import { SourceOrigin } from "./model.js";

export function formatBytes(bytes: number): string {
  const abs = Math.abs(bytes);
  if (abs < 1000) return `${bytes} B`;
  if (abs < 1_000_000) return `${(bytes / 1000).toFixed(1)} KB`;
  if (abs < 1_000_000_000) return `${(bytes / 1_000_000).toFixed(2)} MB`;
  return `${(bytes / 1_000_000_000).toFixed(2)} GB`;
}

export function formatPercent(fraction: number): string {
  if (!Number.isFinite(fraction)) return "-";
  const pct = fraction * 100;
  return pct >= 10 ? `${pct.toFixed(0)}%` : `${pct.toFixed(1)}%`;
}

export function plural(n: number, noun: string): string {
  return `${n} ${noun}${n === 1 ? "" : "s"}`;
}

export const ORIGIN_ORDER: readonly SourceOrigin[] = [
  "app",
  "generated",
  "convex",
  "npm",
  "virtual",
];

export const ORIGIN_COLORS: Record<SourceOrigin, string> = {
  app: "#4fb3d9",
  generated: "#8a8f98",
  convex: "#f3b01c",
  npm: "#c46bd1",
  virtual: "#5e636b",
};

export const ORIGIN_LABELS: Record<SourceOrigin, string> = {
  app: "your code",
  generated: "_generated",
  convex: "convex",
  npm: "npm packages",
  virtual: "shims",
};

export const CODE_COLOR = "#4fb3d9";
export const MAP_COLOR = "#8c8cf0";
export const EXTERNAL_COLOR = "#e59a4f";
export const UNATTRIBUTED_COLOR = "#3f4349";
const TRACK_COLOR = "#3a3d42";
const DIM_COLOR = "#8a8f98";

const PALETTE = [
  "#4fb3d9",
  "#c46bd1",
  "#f3b01c",
  "#6cc36c",
  "#ef6f6c",
  "#8c8cf0",
  "#e59a4f",
  "#46c2a8",
  "#d774a8",
  "#a3b84a",
  "#5f9fe8",
  "#c9a36b",
];

/** A stable color for an arbitrary label (package, component). */
export function colorFor(label: string): string {
  let h = 0;
  for (let i = 0; i < label.length; i++) {
    h = (h * 31 + label.charCodeAt(i)) >>> 0;
  }
  return PALETTE[h % PALETTE.length];
}

export function ownerColor(origin: SourceOrigin, name: string): string {
  return origin === "npm" ? colorFor(name) : ORIGIN_COLORS[origin];
}

const EIGHTHS = ["", "▏", "▎", "▍", "▌", "▋", "▊", "▉"];

/** A horizontal bar `width` cells wide with 1/8-cell resolution. */
export function bar(fraction: number, width: number, color: string): string {
  const clamped = Math.max(0, Math.min(1, fraction));
  const eighths = Math.round(clamped * width * 8);
  const full = Math.floor(eighths / 8);
  const partial = EIGHTHS[eighths % 8];
  const rest = Math.max(0, width - full - (partial ? 1 : 0));
  return (
    chalk.hex(color)("█".repeat(full) + partial) +
    chalk.hex(TRACK_COLOR)("·".repeat(rest))
  );
}

/**
 * One step of a waterfall: a bar `width` cells wide with `value` drawn after
 * `before`, both out of `total`, on a track. A nonzero `value` gets at least
 * one cell, so small steps stay visible.
 */
export function waterfallBar(
  before: number,
  value: number,
  total: number,
  width: number,
  color: string,
): string {
  const scale = width / Math.max(1, total);
  let start = Math.min(width, Math.round(before * scale));
  let end = Math.min(width, Math.round((before + value) * scale));
  if (value > 0 && end === start) {
    if (end < width) end += 1;
    else start -= 1;
  }
  return (
    chalk.hex(TRACK_COLOR)("·".repeat(start)) +
    chalk.hex(color)("█".repeat(end - start)) +
    chalk.hex(TRACK_COLOR)("·".repeat(width - end))
  );
}

const MONOCHROME_GLYPHS = ["█", "▓", "▒", "░", "▞", "="];

/**
 * The fill for the segment at `index` of a stacked bar. Without color support
 * segments cycle through shades so they stay distinguishable.
 */
function segmentGlyph(index: number): string {
  return chalk.level > 0
    ? "█"
    : MONOCHROME_GLYPHS[index % MONOCHROME_GLYPHS.length];
}

/**
 * A bar `width` cells wide split into segments proportional to their values.
 * Cell boundaries are rounded from cumulative sums, so segments always fill
 * exactly `width` cells.
 */
export function stackedBar(
  segments: { value: number; color: string }[],
  width: number,
): string {
  const total = segments.reduce((n, s) => n + s.value, 0);
  if (total === 0) return chalk.hex(TRACK_COLOR)("·".repeat(width));
  let used = 0;
  let acc = 0;
  let out = "";
  segments.forEach((s, idx) => {
    acc += s.value;
    const end = Math.round((acc / total) * width);
    if (end > used) {
      out += chalk.hex(s.color)(segmentGlyph(idx).repeat(end - used));
    }
    used = end;
  });
  return out;
}

/**
 * Truncates to `width` terminal columns, keeping ANSI styles intact. `middle`
 * keeps both ends of a path; `end` keeps the start of a list.
 */
export function truncate(
  s: string,
  width: number,
  position: "middle" | "end" = "end",
): string {
  if (width <= 0) return "";
  if (stringWidth(s) <= width) return s;
  let out = cliTruncate(s, width, { position });
  // cli-truncate can overshoot by a column when the cut lands inside a
  // double-width character.
  for (let w = width - 1; w > 0 && stringWidth(out) > width; w--) {
    out = cliTruncate(s, w, { position });
  }
  return out;
}

export function padEnd(s: string, width: number): string {
  return s + " ".repeat(Math.max(0, width - stringWidth(s)));
}

export function padStart(s: string, width: number): string {
  return " ".repeat(Math.max(0, width - stringWidth(s))) + s;
}

export type Column = {
  header: string;
  align?: "left" | "right";
  /** Flexible columns shrink, with truncation, to fit `maxWidth`. */
  flexible?: boolean;
  truncate?: "middle" | "end";
  /** Dropped when the other columns don't fit `maxWidth` at their minimum. */
  optional?: boolean;
};

const FLEXIBLE_WIDTH_FLOORS = [12, 4];
const COLUMN_GAP = "  ";

export function table(
  columns: Column[],
  rows: string[][],
  maxWidth: number = terminalWidth(),
): string {
  const widths = columns.map((col, idx) =>
    Math.max(
      stringWidth(col.header),
      ...rows.map((r) => stringWidth(r[idx] ?? "")),
    ),
  );
  const flexible = columns.flatMap((c, idx) => (c.flexible ? [idx] : []));
  const total = () =>
    widths.reduce((n, w) => n + w, 0) + COLUMN_GAP.length * (widths.length - 1);
  // Shrink the widest flexible column first, and only take any column below
  // a readable width once every flexible column is at that width. Optional
  // columns go before any flexible column drops below the first floor.
  for (const [idx, floor] of FLEXIBLE_WIDTH_FLOORS.entries()) {
    let excess = total() - maxWidth;
    while (excess > 0 && flexible.length > 0) {
      const widest = flexible.reduce((a, b) => (widths[b] > widths[a] ? b : a));
      if (widths[widest] <= floor) break;
      widths[widest] -= 1;
      excess -= 1;
    }
    if (idx === 0 && total() > maxWidth && columns.some((c) => c.optional)) {
      const keep = columns.flatMap((c, i) => (c.optional ? [] : [i]));
      return table(
        keep.map((i) => columns[i]),
        rows.map((r) => keep.map((i) => r[i])),
        maxWidth,
      );
    }
  }
  const cell = (value: string, idx: number) => {
    const v = truncate(value, widths[idx], columns[idx].truncate ?? "middle");
    return columns[idx].align === "right"
      ? padStart(v, widths[idx])
      : padEnd(v, widths[idx]);
  };
  const lines = [
    chalk.bold(columns.map((c, idx) => cell(c.header, idx)).join(COLUMN_GAP)),
    chalk.hex(TRACK_COLOR)(widths.map((w) => "─".repeat(w)).join(COLUMN_GAP)),
    ...rows.map((r) => r.map((v, idx) => cell(v ?? "", idx)).join(COLUMN_GAP)),
  ];
  return lines.map((l) => truncate(l.trimEnd(), maxWidth)).join("\n");
}

/**
 * Width to lay out tables in. Piped output (scripts, agents) isn't truncated
 * unless COLUMNS is set, so no information is lost.
 */
export function terminalWidth(): number {
  const fromEnv = parseInt(process.env.COLUMNS ?? "", 10);
  if (Number.isFinite(fromEnv) && fromEnv > 0) return fromEnv;
  if (!process.stdout.isTTY) return Number.POSITIVE_INFINITY;
  return process.stdout.columns || 100;
}

/**
 * Cells for a bar column in a table `width` wide whose other columns need
 * about `reserved` cells: `max` when there's room, down to a minimum of 6.
 */
export function barCells(width: number, reserved: number, max: number) {
  return Math.max(6, Math.min(max, width - reserved));
}

/** Width for bars, which need a finite size even when output is piped. */
export function layoutWidth(): number {
  return Math.min(terminalWidth(), 100);
}

export function heading(text: string): string {
  return `\n${chalk.bold(truncate(text, terminalWidth()))}\n`;
}

export function dim(text: string): string {
  return chalk.hex(DIM_COLOR)(text);
}

export function colored(color: string, text: string): string {
  return chalk.hex(color)(text);
}

export function bold(text: string): string {
  return chalk.bold(text);
}

/** Word-wraps `text` to `width` columns. */
export function prose(text: string, width: number): string {
  return wrapItems(text.split(" "), " ", width);
}

/**
 * Joins `items` with `separator`, starting a new line (with `prefix`) before
 * any item that would overflow `width`. Items are never split.
 */
export function wrapItems(
  items: string[],
  separator: string,
  width: number,
  prefix = "",
): string {
  const lines: string[] = [];
  let line = "";
  for (const item of items) {
    const candidate = line === "" ? prefix + item : line + separator + item;
    if (line !== "" && stringWidth(candidate) > width) {
      lines.push(line);
      line = prefix + item;
    } else {
      line = candidate;
    }
  }
  if (line !== "") lines.push(line);
  return lines.join("\n");
}

/** A key for a stacked bar drawn from the same `entries`, in the same order. */
export function legend(
  entries: { label: string; color: string; value?: string }[],
  width: number,
  prefix = "",
): string {
  return wrapItems(
    entries.map(
      (e, idx) =>
        `${chalk.hex(e.color)(segmentGlyph(idx))} ${e.label}${e.value ? dim(` ${e.value}`) : ""}`,
    ),
    "   ",
    width,
    prefix,
  );
}

export function indent(text: string, prefix = "  "): string {
  return text
    .split("\n")
    .map((l) => (l.length > 0 ? prefix + l : l))
    .join("\n");
}
