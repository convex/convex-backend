import readline from "readline";
import chalk from "chalk";
import stringWidth from "string-width";
import {
  AnalyzedBuild,
  AnalyzedOutput,
  CODE_GLUE_NAME,
  MAP_OVERHEAD_NAME,
  UNMATCHED_SOURCES_NAME,
  displayPath,
  distinctExternalPackages,
} from "./model.js";
import { moduleLoads } from "./modules.js";
import { Grouping, TreeNode, tree } from "./tree.js";
import { importChains } from "./importChains.js";
import {
  bar,
  formatBytes,
  formatPercent,
  padEnd,
  padStart,
  plural,
  truncate,
  wrapItems,
} from "./format.js";
import { nodeColor, renderTreemap } from "./treemap.js";

type Level = {
  node: TreeNode;
  selected: number;
  scroll: number;
  color: string | undefined;
};

type Layout = "split" | "treemap" | "list";
const LAYOUTS: readonly Layout[] = ["split", "treemap", "list"];

const CHROME_BG = "#1f2227";
const SELECTED_BG = "#2a2e35";
const ACCENT = "#f3b01c";
const MUTED = "#8a8f98";
const FAINT = "#5e636b";
const RULE = "#3a3d42";

/** Narrower than this, the split layout shows only the list. */
const MIN_SPLIT_WIDTH = 100;
const CHROME_ROWS = 4;
const MAX_NAME_WIDTH = 72;

export type KeyResult = "quit" | undefined;

/** How a non-entry chunk gets loaded, from the modules that import it. */
export function chunkDescription(build: AnalyzedBuild, path: string): string {
  const importers = build.outputs.filter((o) => o.imports.includes(path));
  const lazyImporters = build.outputs.filter((o) =>
    o.dynamicImports.includes(path),
  );
  const list = (outputs: AnalyzedOutput[]) =>
    outputs.length <= 2
      ? outputs.map((o) => o.path).join(" and ")
      : `${outputs.length} modules`;
  if (importers.length === 0) {
    return lazyImporters.length === 0
      ? "Split chunk."
      : `Loaded lazily, by import() in ${list(lazyImporters)}.`;
  }
  const lazily =
    lazyImporters.length > 0 ? `, and lazily by ${list(lazyImporters)}` : "";
  return `Shared chunk, imported by ${list(importers)}${lazily}.`;
}

/** Children of the unmatched sources node are named by their source. */
function glueDescription(name: string): string {
  switch (name) {
    case CODE_GLUE_NAME:
      return "Code esbuild emitted that no input file accounts for (e.g. runtime helpers and module wrappers)";
    case MAP_OVERHEAD_NAME:
      return "Source map bytes that belong to no single file (e.g. the names list, JSON syntax, and unmapped segments)";
    case UNMATCHED_SOURCES_NAME:
      return "Source map bytes for original files the analyzer couldn't match to a bundled input.";
    default:
      return "Source map bytes for this original file, which the analyzer couldn't match to a bundled input.";
  }
}

/**
 * State and rendering for `npx @convex-dev/bundle-analyzer explore`,
 * independent of the terminal so frames can be rendered in tests.
 */
export class Explorer {
  private grouping: Grouping = "component";
  private layout: Layout = "split";
  private stack: Level[] = [];
  private filter = "";
  private filtering = false;
  private showHelp = false;
  /** Rows PgUp/PgDn move by, from the last rendered frame. */
  private pageSize = 10;
  private moduleLoadByKey = new Map<string, number>();
  private externals: ReturnType<typeof distinctExternalPackages>;
  private chainCache = new Map<string, Chain | null>();

  constructor(private builds: AnalyzedBuild[]) {
    this.externals = distinctExternalPackages(builds);
    this.rebuild([]);
  }

  /**
   * Recomputes the tree for the current grouping, then drills back
   * down to `path` (child names below the root) as far as it still exists.
   */
  private rebuild(path: string[], selectedName?: string) {
    this.moduleLoadByKey = new Map(
      moduleLoads(this.builds).map((m) => [
        `${m.buildId}\0${m.path}`,
        m.loadBytes,
      ]),
    );
    const root = tree(this.builds, this.grouping);
    this.stack = [{ node: root, selected: 0, scroll: 0, color: undefined }];
    for (const name of path) {
      const idx = this.level.node.children.findIndex((c) => c.name === name);
      if (idx === -1) break;
      this.level.selected = idx;
      this.enter(this.level.node.children[idx]);
    }
    if (selectedName !== undefined) {
      const idx = this.level.node.children.findIndex(
        (c) => c.name === selectedName,
      );
      this.level.selected = Math.max(0, idx);
    }
  }

  private get level(): Level {
    return this.stack[this.stack.length - 1];
  }

  private enter(node: TreeNode) {
    this.stack.push({
      node,
      selected: 0,
      scroll: 0,
      color: nodeColor(node, this.level.color),
    });
  }

  private visibleChildren(): TreeNode[] {
    const children = this.level.node.children;
    if (!this.filter) return children;
    const q = this.filter.toLowerCase();
    return children.filter((c) => c.name.toLowerCase().includes(q));
  }

  private selectedChild(): TreeNode | undefined {
    return this.visibleChildren()[this.level.selected];
  }

  handleKey(str: string | undefined, key: readline.Key | undefined): KeyResult {
    const name = key?.name ?? str;
    if (key?.ctrl && key.name === "c") return "quit";
    if (this.filtering) {
      this.filterKey(str, key);
      return;
    }
    if (this.showHelp) {
      this.showHelp = false;
      return;
    }
    const lvl = this.level;
    const count = this.visibleChildren().length;
    const page = Math.max(1, this.pageSize);
    const move = (to: number) => {
      lvl.selected = Math.max(0, Math.min(count - 1, to));
    };
    switch (name) {
      case "q":
        return "quit";
      case "up":
      case "k":
        move(lvl.selected - 1);
        break;
      case "down":
      case "j":
        move(lvl.selected + 1);
        break;
      case "pageup":
        move(lvl.selected - page);
        break;
      case "pagedown":
        move(lvl.selected + page);
        break;
      case "home":
        move(0);
        break;
      case "end":
        move(count - 1);
        break;
      case "right":
      case "l":
      case "return":
      case "enter": {
        const child = this.selectedChild();
        if (child && child.children.length > 0) {
          this.enter(child);
          this.filter = "";
        }
        break;
      }
      case "left":
      case "h":
      case "backspace":
        this.up();
        break;
      case "escape":
        if (this.filter) this.filter = "";
        else this.up();
        break;
      case "t":
        this.layout = LAYOUTS[(LAYOUTS.indexOf(this.layout) + 1) % 3];
        break;
      case "g":
        this.grouping = this.grouping === "component" ? "package" : "component";
        this.filter = "";
        this.rebuild([]);
        break;
      case "/":
        this.filtering = true;
        this.filter = "";
        break;
      case "?":
        this.showHelp = true;
        break;
    }
    return;
  }

  private filterKey(str: string | undefined, key: readline.Key | undefined) {
    if (key?.name === "return" || key?.name === "enter") {
      this.filtering = false;
    } else if (key?.name === "escape") {
      this.filtering = false;
      this.filter = "";
    } else if (key?.name === "backspace") {
      this.filter = this.filter.slice(0, -1);
    } else if (str && !key?.ctrl && !key?.meta && stringWidth(str) > 0) {
      this.filter += str;
    }
    this.level.selected = 0;
    this.level.scroll = 0;
  }

  private up() {
    if (this.stack.length > 1) {
      this.stack.pop();
      this.filter = "";
    }
  }

  /** The screen as `height` rows, each at most `width` columns wide. */
  frame(width: number, height: number): string[] {
    const bodyHeight = Math.max(1, height - CHROME_ROWS);
    this.pageSize = bodyHeight - 2;
    const lines = [
      this.titleBar(width),
      this.breadcrumb(width),
      ...(this.showHelp
        ? this.help(width, bodyHeight)
        : this.body(width, bodyHeight)),
    ];
    while (lines.length < height - 2) lines.push("");
    lines.length = Math.max(0, height - 2);
    lines.push(this.statusLine(width), this.keyHints(width));
    return lines.slice(0, height).map((l) => truncate(l, width));
  }

  private titleBar(width: number): string {
    let right = ` Total size ${formatBytes(this.stack[0].node.bytes)} `;
    let left = " ◆ Convex bundle explorer ";
    let middle = ` ${this.grouping === "component" ? "by component" : "by package"} · code + maps${this.externals.some((pkg) => pkg.diskBytes !== null && pkg.diskBytes > 0) ? " + externals (est.)" : ""} `;
    const room = () =>
      width - stringWidth(left) - stringWidth(middle) - stringWidth(right);
    if (room() < 0) middle = "";
    if (room() < 0) left = " ◆ ";
    if (room() < 0) right = ` ${formatBytes(this.stack[0].node.bytes)} `;
    return (
      chalk.bgHex(ACCENT).hex("#111111").bold(left) +
      chalk.bgHex(CHROME_BG).hex("#dddddd")(
        middle + " ".repeat(Math.max(0, room())),
      ) +
      chalk.bgHex(CHROME_BG).hex(ACCENT).bold(right)
    );
  }

  private breadcrumb(width: number): string {
    const parts = this.stack.map((l, idx) => {
      const name = idx === 0 ? "bundle" : l.node.name;
      const style = chalk.hex(l.color ?? "#dddddd");
      return idx === this.stack.length - 1 ? style.bold(name) : style(name);
    });
    const total = Math.max(1, this.stack[0].node.bytes);
    const size = chalk.hex(MUTED)(
      `  ${formatBytes(this.level.node.bytes)} · ${formatPercent(this.level.node.bytes / total)} of total`,
    );
    const filter =
      this.filtering || this.filter
        ? chalk.hex(ACCENT)(
            `  filter: ${this.filter}${this.filtering ? "▌" : ""}`,
          )
        : "";
    // The end of the path is the part being looked at, so drop from the front.
    const budget = Math.max(
      10,
      width - 1 - stringWidth(size) - stringWidth(filter),
    );
    const sep = chalk.hex(FAINT)(" › ");
    let shown = parts;
    let crumb = shown.join(sep);
    while (shown.length > 1 && stringWidth(crumb) > budget) {
      shown = shown.slice(1);
      crumb = chalk.hex(FAINT)("…") + sep + shown.join(sep);
    }
    return " " + truncate(crumb, budget) + size + filter;
  }

  private body(width: number, height: number): string[] {
    const children = this.visibleChildren();
    const lvl = this.level;
    lvl.selected = Math.max(0, Math.min(lvl.selected, children.length - 1));
    const scoped = { ...lvl.node, children };
    if (this.layout === "treemap") {
      return this.treemap(scoped, width, height);
    }
    const split = this.layout === "split" && width >= MIN_SPLIT_WIDTH;
    const listWidth = split ? Math.floor(width * 0.5) : width;
    const list = this.list(children, listWidth, height);
    if (!split) return list;

    const rightWidth = width - listWidth - 1;
    const detailsHeight = Math.max(6, Math.min(14, Math.floor(height * 0.4)));
    const mapHeight = Math.max(1, height - detailsHeight - 1);
    const map = this.treemap(scoped, rightWidth, mapHeight);
    while (map.length < mapHeight) map.push("");
    const right = [
      ...map,
      chalk.hex(RULE)("─".repeat(rightWidth)),
      ...this.details(this.selectedChild(), rightWidth, detailsHeight),
    ];
    const sep = chalk.hex(RULE)("│");
    const rows: string[] = [];
    for (let i = 0; i < height; i++) {
      rows.push(padEnd(list[i] ?? "", listWidth) + sep + (right[i] ?? ""));
    }
    return rows;
  }

  private treemap(node: TreeNode, width: number, height: number): string[] {
    if (node.children.length === 0) {
      return [chalk.hex(MUTED)(this.emptyMessage())];
    }
    return renderTreemap(node, {
      width,
      height,
      depth: 2,
      highlight: this.level.selected,
      color: this.level.color,
    });
  }

  private emptyMessage(): string {
    return this.filter ? "  no matches" : "  (empty)";
  }

  private list(children: TreeNode[], width: number, height: number): string[] {
    const lvl = this.level;
    const parentBytes = Math.max(1, lvl.node.bytes);
    const sizeWidth = 10;
    const pctWidth = 6;
    const barWidth = Math.max(4, Math.min(20, Math.floor(width * 0.2)));
    // Marker, swatch and space before the name; space before the bar and the
    // drill-down arrow after it.
    const fixed = 3 + sizeWidth + pctWidth + 1 + barWidth + 2;
    // Capped so sizes stay next to their names on wide terminals.
    const nameWidth = Math.max(6, Math.min(MAX_NAME_WIDTH, width - fixed));
    const visibleRows = Math.max(1, height - 1);
    if (lvl.selected < lvl.scroll) lvl.scroll = lvl.selected;
    if (lvl.selected >= lvl.scroll + visibleRows) {
      lvl.scroll = lvl.selected - visibleRows + 1;
    }
    const position =
      children.length > visibleRows
        ? `${lvl.selected + 1}/${children.length}`
        : "";
    const rows = [
      chalk.hex(FAINT)(
        "   " +
          padEnd("name", nameWidth) +
          padStart("size", sizeWidth) +
          padStart("%", pctWidth) +
          " " +
          padStart(position, barWidth),
      ),
    ];
    children.slice(lvl.scroll, lvl.scroll + visibleRows).forEach((child, i) => {
      const selected = lvl.scroll + i === lvl.selected;
      const color = nodeColor(child, lvl.color);
      const fraction = child.bytes / parentBytes;
      const name = truncate(child.name, nameWidth, "middle");
      const text =
        (selected ? chalk.hex(ACCENT)("▌") : " ") +
        chalk.hex(color)("■") +
        " " +
        padEnd(selected ? chalk.bold(name) : name, nameWidth) +
        padStart(formatBytes(child.bytes), sizeWidth) +
        chalk.hex(MUTED)(padStart(formatPercent(fraction), pctWidth)) +
        " " +
        bar(fraction, barWidth, color) +
        " " +
        chalk.hex(FAINT)(child.children.length > 0 ? "›" : " ");
      rows.push(
        selected ? chalk.bgHex(SELECTED_BG)(padEnd(text, width)) : text,
      );
    });
    if (children.length === 0) rows.push(chalk.hex(MUTED)(this.emptyMessage()));
    return rows;
  }

  private details(
    node: TreeNode | undefined,
    width: number,
    height: number,
  ): string[] {
    if (!node) return [];
    const color = nodeColor(node, this.level.color);
    const total = Math.max(1, this.stack[0].node.bytes);
    const parent = Math.max(1, this.level.node.bytes);
    const lines = [
      chalk.hex(color).bold(truncate(node.name, width, "middle")),
      chalk.hex(MUTED)(
        truncate(
          `${KIND_LABELS[node.kind]} · ${formatBytes(node.bytes)} · ${formatPercent(node.bytes / parent)} of parent · ${formatPercent(node.bytes / total)} of total`,
          width,
        ),
      ),
    ];
    if (node.kind === "external") {
      lines.push(
        ...wrapItems(
          "Installed size estimated from local node_modules; server size may differ. Counted once per package version.".split(
            " ",
          ),
          " ",
          width,
        )
          .split("\n")
          .map((line) => chalk.hex(MUTED)(line)),
      );
    }
    if (node.kind === "overhead") {
      lines.push(
        ...wrapItems(
          "Code and source map bytes that cannot be attributed to a package.".split(
            " ",
          ),
          " ",
          width,
        )
          .split("\n")
          .map((line) => chalk.hex(MUTED)(line)),
      );
    }
    if (node.kind === "glue") {
      lines.push(chalk.hex(MUTED)(glueDescription(node.name)));
    }
    if (node.kind === "module" && node.buildId) {
      const load = this.moduleLoadByKey.get(`${node.buildId}\0${node.name}`);
      const build = this.builds.find((b) => b.id === node.buildId);
      lines.push(
        load !== undefined
          ? `Loading it loads ${chalk.bold(formatBytes(load))} of code, including the shared chunks it imports.`
          : chalk.hex(MUTED)(
              truncate(
                build ? chunkDescription(build, node.name) : "Split chunk.",
                width,
              ),
            ),
      );
    }
    if (node.kind === "file" && node.inputPath && node.buildId) {
      const build = this.builds.find((b) => b.id === node.buildId);
      lines.push(
        chalk.hex(MUTED)(
          truncate(
            displayPath(node.inputPath, build?.attribution.get(node.inputPath)),
            width,
            "middle",
          ),
        ),
      );
      const chain = this.chain(node.buildId, node.inputPath);
      if (chain) {
        lines.push(
          chalk.hex(ACCENT)(
            `Imported by ${plural(chain.entryPoints, "entry point")}, for example:`,
          ),
        );
        chain.steps.forEach((step, idx) => {
          const depth = "  ".repeat(Math.min(idx, 6));
          lines.push(
            truncate(
              `${depth}${idx === 0 ? "◉" : "└"} ${step}`,
              width,
              "middle",
            ),
          );
        });
      }
    }
    const room = height - lines.length - 1;
    if (node.children.length > 0 && room > 0) {
      lines.push(chalk.hex(FAINT)("Largest inside:"));
      for (const c of node.children.slice(0, room)) {
        lines.push(
          truncate(
            `  ${chalk.hex(nodeColor(c, color))("■")} ${padStart(formatBytes(c.bytes), 9)}  ${c.name}`,
            width,
          ),
        );
      }
    }
    return lines.slice(0, height).map((l) => truncate(l, width));
  }

  /** The shortest import chain from an entry point to a file, cached. */
  private chain(buildId: string, inputPath: string): Chain | null {
    const key = `${buildId}\0${inputPath}`;
    const cached = this.chainCache.get(key);
    if (cached !== undefined) return cached;
    const build = this.builds.find((b) => b.id === buildId);
    let result: Chain | null = null;
    if (build) {
      const matches = importChains([build], {
        kind: "files",
        inputPaths: new Set([inputPath]),
      });
      if (matches.length > 0) {
        result = {
          entryPoints: matches.length,
          steps: matches[0].chain.map((s) => s.display),
        };
      }
    }
    this.chainCache.set(key, result);
    return result;
  }

  private help(width: number, height: number): string[] {
    const keys: [string, string][] = [
      ["↑ ↓  j k", "move the selection"],
      ["→  l  enter", "open the selection"],
      ["←  h  backspace", "go up a level"],
      ["PgUp PgDn Home End", "jump"],
      ["t", "layout: split, treemap only, list only"],
      ["g", "group by component or by package"],
      ["/", "filter this level (enter keeps it, esc clears it)"],
      ["esc", "clear the filter, or go up a level"],
      ["q  ctrl-c", "quit"],
    ];
    const lines = ["", chalk.bold("  Keys"), ""];
    for (const [k, d] of keys) {
      lines.push(`  ${chalk.hex(ACCENT)(padEnd(k, 22))}${d}`);
    }
    lines.push(
      "",
      chalk.hex(MUTED)(
        "  Total size includes code, source maps, and installed external package estimates.",
      ),
      chalk.hex(MUTED)(
        "  Source map sizes are the bytes of each file's mappings, source and path.",
      ),
      chalk.hex(MUTED)(
        `  ${CODE_GLUE_NAME} and ${MAP_OVERHEAD_NAME} are bytes that belong to no single file;`,
      ),
      chalk.hex(MUTED)(
        `  ${UNMATCHED_SOURCES_NAME} are source map bytes for files the analyzer couldn't match to an input.`,
      ),
      chalk.hex(MUTED)(
        "  Package grouping puts those bytes in a separate unattributed branch. Both groupings have the same total.",
      ),
      "",
      chalk.hex(FAINT)("  Press any key to go back."),
    );
    return lines.slice(0, height).map((l) => truncate(l, width));
  }

  private statusLine(width: number): string {
    const child = this.selectedChild();
    const text = child
      ? ` ${child.name} · ${formatBytes(child.bytes)}${child.children.length > 0 ? ` · ${plural(child.children.length, "item")}` : ""}`
      : " ";
    const missing = this.externals.filter(
      (pkg) => pkg.diskBytes === null,
    ).length;
    const estimate =
      missing > 0
        ? ` ${plural(missing, "external package")} unmeasured; excluded from total ·`
        : "";
    return chalk.bgHex(CHROME_BG).hex("#cccccc")(
      padEnd(truncate(estimate + text, width, "middle"), width),
    );
  }

  private keyHints(width: number): string {
    const navigation: [string, string][] = [
      ["↑↓", "move"],
      ["→", "open"],
      ["←", "back"],
    ];
    const toggles: [string, string][] = [
      ["t", this.layout],
      ["g", this.grouping],
      ["/", "filter"],
      ["?", "help"],
      ["q", "quit"],
    ];
    const render = (hints: [string, string][]) =>
      " " +
      hints
        .map(
          ([k, d]) =>
            `${chalk.bgHex(RULE).hex("#ffffff")(` ${k} `)} ${chalk.hex("#aaaaaa")(d)}`,
        )
        .join(" ");
    // Arrow keys are easy to guess without a hint, so they're dropped first.
    const full = render([...navigation, ...toggles]);
    return stringWidth(full) <= width ? full : render(toggles);
  }
}

type Chain = { entryPoints: number; steps: string[] };

const KIND_LABELS: Record<TreeNode["kind"], string> = {
  root: "bundle",
  external: "installed external estimate",
  overhead: "unattributed bytes",
  component: "component",
  build: "build",
  module: "module",
  owner: "package",
  file: "file",
  dir: "directory",
  glue: "unattributed",
};

/**
 * Runs the explorer in the alternate screen until the user quits. The
 * terminal is restored on quit, on an error while handling input (which is
 * then rethrown), on SIGTERM/SIGHUP, and on process exit.
 */
export async function runExplorer(builds: AnalyzedBuild[]): Promise<void> {
  const { stdin, stdout } = process;
  // The treemap is drawn with background colors, so it needs at least basic
  // color support even when chalk detected none.
  if (chalk.level === 0) chalk.level = 1;
  const explorer = new Explorer(builds);

  await new Promise<void>((resolve, reject) => {
    let finished = false;
    const restore = () => {
      stdout.write("\x1b[0m\x1b[?25h\x1b[?1049l");
      if (stdin.isTTY) stdin.setRawMode(false);
    };
    const finish = (error?: unknown) => {
      if (finished) return;
      finished = true;
      stdin.off("keypress", onKey);
      stdout.off("resize", draw);
      process.off("exit", restore);
      for (const signal of SIGNALS) process.off(signal, onSignal);
      restore();
      stdin.pause();
      if (error === undefined) resolve();
      else reject(error);
    };
    const guard =
      <A extends unknown[]>(fn: (...args: A) => void) =>
      (...args: A) => {
        try {
          fn(...args);
        } catch (error) {
          finish(error);
        }
      };
    const draw = guard(() => {
      const width = stdout.columns || 100;
      const height = stdout.rows || 30;
      stdout.write(
        explorer
          .frame(width, height)
          .map((line, row) => {
            // Clearing to the end of a full-width line would erase its last
            // cell, since the cursor stays on that cell.
            const clear = stringWidth(line) < width ? "\x1b[K" : "";
            return `\x1b[${row + 1};1H${line}\x1b[0m${clear}`;
          })
          .join(""),
      );
    });
    const onKey = guard((str: string | undefined, key: readline.Key) => {
      if (explorer.handleKey(str, key) === "quit") finish();
      else draw();
    });
    const onSignal = (signal: NodeJS.Signals) => {
      finish();
      process.kill(process.pid, signal);
    };

    readline.emitKeypressEvents(stdin);
    stdin.setRawMode(true);
    stdin.resume();
    stdin.on("keypress", onKey);
    stdout.on("resize", draw);
    process.on("exit", restore);
    for (const signal of SIGNALS) process.once(signal, onSignal);
    stdout.write("\x1b[?1049h\x1b[?25l\x1b[2J");
    draw();
  });
}

const SIGNALS: NodeJS.Signals[] = ["SIGTERM", "SIGHUP"];
