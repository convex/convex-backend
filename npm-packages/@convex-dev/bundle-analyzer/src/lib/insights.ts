import fs from "node:fs";
import path from "node:path";
import { AnalyzedBuild, ownerLabel } from "./model.js";
import {
  bold,
  colored,
  dim,
  formatBytes,
  plural,
  terminalWidth,
  truncate,
  wrapItems,
} from "./format.js";
import {
  Dominators,
  InputGraph,
  sizesOnlyThrough,
  sizesByInput,
  dominators,
  inputGraph,
  reachable,
} from "./graph.js";
import { directorySize } from "./collect.js";
import { EXTERNAL_PACKAGES_DOCS } from "./command.js";
import { isFunctionRuntime } from "./modules.js";
import { targetUse } from "./usage.js";
import { Sizes, sizes, sumSizes } from "./sizes.js";
import { packageUsage } from "./packages.js";

const LEVEL_ICONS: Record<Insight["level"], [string, string]> = {
  warn: ["#f3b01c", "▲"],
  info: ["#4fb3d9", "●"],
};

export function insightsReport(
  found: Insight[],
  width = terminalWidth(),
): string {
  return found
    .map((i) => {
      const [color, icon] = LEVEL_ICONS[i.level];
      const title = wrapItems(
        i.title.split(" "),
        " ",
        width,
        "    ",
      ).trimStart();
      const detail = wrapItems(i.detail.split(" "), " ", width, "    ");
      return [
        `  ${colored(color, icon)} ${bold(title)}`,
        dim(detail),
        ...rowLines(i.rows ?? [], width),
      ].join("\n");
    })
    .join("\n");
}

/** Each item with its attributed sizes and suggested fix wrapped underneath. */
function rowLines(rows: InsightRow[], width: number): string[] {
  const shown = rows.slice(0, MAX_LISTED_ROWS);
  const lines = shown.flatMap((r) => [
    truncate(`      ${r.text}`, width, "middle"),
    dim(wrapItems(formatSizes(r).split(" "), " ", width, "        ")),
    ...(r.note
      ? [dim(wrapItems(r.note.split(" "), " ", width, "        "))]
      : []),
  ]);
  if (rows.length > shown.length) {
    lines.push(dim(`      … ${rows.length - shown.length} more (use --json)`));
  }
  return lines;
}

/** One line per insight, title only, for the summary. */
export function insightTitles(
  found: Insight[],
  width = terminalWidth(),
): string {
  return found
    .map((i) => {
      const [color, icon] = LEVEL_ICONS[i.level];
      const title = wrapItems(
        i.title.split(" "),
        " ",
        width,
        "    ",
      ).trimStart();
      return `  ${colored(color, icon)} ${title}`
        .split("\n")
        .map((line) => truncate(line, width, "middle"))
        .join("\n");
    })
    .join("\n");
}

/** Names in the project's package.json `devDependencies`. */
export function readDevDependencies(projectRoot: string): Set<string> {
  try {
    const pkg = JSON.parse(
      fs.readFileSync(path.join(projectRoot, "package.json"), "utf8"),
    );
    return new Set(Object.keys(pkg.devDependencies ?? {}));
  } catch {
    return new Set();
  }
}

export type InsightKind =
  | "duplicate-package"
  | "alternate-package-implementations"
  | "use-node-file-in-convex-runtime"
  | "package-in-both-runtimes"
  | "external-package-candidate"
  | "dev-dependency-bundled"
  | "test-file-bundled"
  | "package-in-many-components";

/** A file, package, package copy or component an insight covers. */
export type InsightItem = Sizes & {
  subject: string;
  /** Your files that pull a package into a build. */
  importedBy?: string[];
  /** The leaked input is also a function module in this build. */
  entryPoint?: boolean;
  /**
   * For a "use node" file, how to fix each file without "use node" that
   * imports it: add "use node" to it, or move what it uses out of the
   * "use node" file.
   */
  fixes?: { file: string; fix: "add-use-node" | "move" }[];
};

export type Insight = {
  kind: InsightKind;
  level: "warn" | "info";
  title: string;
  detail: string;
  /**
   * Combined code and source map savings. External packages have zero savings
   * because their installed size also counts toward deployment size.
   */
  savedBytes: number;
  savedCodeBytes: number;
  savedSourceMapBytes: number;
  buildIds: string[];
  /** The package (`name@version`) or module path the insight is about. */
  subject: string | null;
  /** What makes up `savedBytes`, largest first. */
  items: InsightItem[];
  /** Rows printed under the detail, largest first. */
  rows?: InsightRow[];
};

/** A row printed under an insight's detail: a size, then what it's for. */
export type InsightRow = Sizes & {
  /** Truncated to one line. */
  text: string;
  /** Wrapped under `text`. */
  note?: string;
};

// Absolute sizes: deployment limits are enforced (and can change) server side.
const DUPLICATE_MIN_EXTRA_BYTES = 10_000;
const SHARED_PACKAGE_MIN_BYTES = 50_000;
const BOTH_RUNTIMES_MIN_BYTES = 500_000;
const EXTERNAL_PACKAGE_MIN_BYTES = 500_000;
const SHARED_PACKAGE_MIN_COMPONENTS = 3;
const DEV_DEPENDENCY_MIN_BYTES = 10_000;
/** How many items a detail names before "and N more". */
const MAX_LISTED_ITEMS = 5;
/** How many rows an insight lists before "… N more". */
const MAX_LISTED_ROWS = 10;

/**
 * Suggestions for shrinking the bundle, largest `savedBytes` first. Every
 * size counts code and source maps, matching the package rankings.
 * `devDependencies` are the names in the project's package.json. With
 * `projectRoot`, external package suggestions give each package's installed
 * size.
 */
export function findInsights(
  builds: AnalyzedBuild[],
  devDependencies: ReadonlySet<string> = new Set(),
  projectRoot?: string,
): Insight[] {
  const graphs = new BuildGraphs();
  const found = [
    ...duplicateInsights(builds),
    ...alternateImplementationInsights(builds, graphs),
    ...nodeFileInsights(builds, graphs),
    ...bothRuntimesInsights(builds, graphs),
    ...externalPackageInsights(builds, projectRoot),
    ...devCodeInsights(builds, devDependencies, graphs),
    ...sharedPackageInsights(builds),
  ];
  const order = { warn: 0, info: 1 };
  return found.sort(
    (a, b) =>
      b.savedBytes - a.savedBytes ||
      order[a.level] - order[b.level] ||
      a.title.localeCompare(b.title),
  );
}

/** "a", "a and b", "a, b and c". */
function joinAnd(items: string[]): string {
  if (items.length <= 1) return items.join("");
  return `${items.slice(0, -1).join(", ")} and ${items[items.length - 1]}`;
}

/** The first `max` files, then how many others there are. */
function fileList(files: string[], max = 2): string {
  return joinAnd([
    ...files.slice(0, max),
    ...(files.length > max ? [plural(files.length - max, "other file")] : []),
  ]);
}

/** The largest items with their sizes, then how many more there are. */
function listItems(items: InsightItem[], max = MAX_LISTED_ITEMS): string {
  const shown = items
    .slice(0, max)
    .map((i) => `${i.subject} (${formatSizes(i)})`);
  if (items.length > max) shown.push(`${items.length - max} more`);
  return joinAnd(shown);
}

/**
 * Each build's whole import graph, with `import()` edges since lazily loaded
 * code is bundled too, rooted at its function modules.
 */
class BuildGraphs {
  private cache = new Map<
    string,
    {
      graph: InputGraph;
      roots: string[];
      bytes: Map<string, Sizes>;
      doms: Dominators;
    }
  >();

  get(b: AnalyzedBuild) {
    let entry = this.cache.get(b.id);
    if (!entry) {
      const graph = inputGraph(b);
      const roots = b.outputs.flatMap((o) =>
        o.entryPoint ? [o.entryPoint] : [],
      );
      const bytes = sizesByInput(b.outputs);
      entry = { graph, roots, bytes, doms: dominators(graph, roots, bytes) };
      this.cache.set(b.id, entry);
    }
    return entry;
  }
}

/** Copies of a package in one or more builds, merged when the same dirs repeat. */
function duplicateInsights(builds: AnalyzedBuild[]): Insight[] {
  const groups = new Map<
    string,
    {
      name: string;
      buildIds: string[];
      extra: Sizes;
      copies: Map<string, { version: string | null } & Sizes>;
    }
  >();
  for (const d of duplicates(builds)) {
    const key = [d.name, ...d.copies.map((c) => c.packageDir).sort()].join(
      "\0",
    );
    let g = groups.get(key);
    if (!g) {
      g = { name: d.name, buildIds: [], extra: sizes(), copies: new Map() };
      groups.set(key, g);
    }
    g.buildIds.push(d.buildId);
    g.extra = sumSizes([g.extra, ...d.copies.slice(1)]);
    for (const c of d.copies) {
      const copy = g.copies.get(c.packageDir) ?? {
        version: c.version,
        ...sizes(),
      };
      Object.assign(copy, sumSizes([copy, c]));
      g.copies.set(c.packageDir, copy);
    }
  }
  const result: Insight[] = [];
  for (const g of groups.values()) {
    if (g.extra.bytes < DUPLICATE_MIN_EXTRA_BYTES) continue;
    const copies = [...g.copies.entries()]
      .map(([packageDir, c]) => ({ packageDir, ...c }))
      .sort((x, y) => y.bytes - x.bytes);
    const sameVersion =
      new Set(copies.map((c) => c.version)).size < copies.length;
    const listed = copies
      .map((c) => {
        const where = sameVersion
          ? ` in ${c.packageDir.replace(/^(\.\.\/)+/, "")}`
          : "";
        return `${c.version ?? "?"}${where} (${formatSizes(c)})`;
      })
      .join(", ");
    result.push({
      kind: "duplicate-package",
      level: "warn",
      title: `${g.name} is bundled ${copies.length} times in ${joinAnd(g.buildIds)}, adding ${formatSizes(g.extra)}`,
      detail: `Copies: ${listed}. Aligning the versions your dependencies need, or deduplicating (e.g. \`npm dedupe\`), keeps one. Use \`npx @convex-dev/bundle-analyzer packages ${g.name}\` to see what imports each copy.`,
      ...savings(g.extra),
      buildIds: g.buildIds,
      subject: g.name,
      items: copies.map((c) => ({
        subject: c.packageDir,
        ...sizes(c.codeBytes, c.sourceMapBytes),
      })),
    });
  }
  return result;
}

/**
 * Matching paths under ESM/CommonJS directories, or with JS module-format
 * extensions, are candidates for alternate implementations in one install.
 * Independent import paths distinguish implementations from format wrappers.
 */
function alternateImplementationInsights(
  builds: AnalyzedBuild[],
  graphs: BuildGraphs,
): Insight[] {
  const result: Insight[] = [];
  for (const b of builds) {
    const packages = new Map<
      string,
      { label: string; dir: string; paths: Map<string, Map<string, string>> }
    >();
    for (const [file, value] of sizesByInput(b.outputs)) {
      const a = b.attribution.get(file);
      if (value.codeBytes === 0 || a?.origin !== "npm" || !a.packageDir)
        continue;
      const relative = path.posix.relative(a.packageDir, file);
      const variant = implementationVariant(relative);
      if (!variant) continue;
      const key = `${ownerLabel(a)}\0${a.packageDir}`;
      let pkg = packages.get(key);
      if (!pkg) {
        pkg = { label: ownerLabel(a), dir: a.packageDir, paths: new Map() };
        packages.set(key, pkg);
      }
      const formats = pkg.paths.get(variant.path) ?? new Map<string, string>();
      formats.set(variant.format, file);
      pkg.paths.set(variant.path, formats);
    }
    for (const pkg of packages.values()) {
      const { graph, roots, bytes } = graphs.get(b);
      const variants = new Map<string, Set<string>>();
      for (const formats of pkg.paths.values()) {
        if (formats.size < 2) continue;
        const paths = [...formats.values()];
        if (
          paths.some((p) => {
            const imports = reachable(graph, [p]);
            return paths.some((other) => other !== p && imports.has(other));
          })
        )
          continue;
        for (const [format, file] of formats) {
          const inputs = variants.get(format) ?? new Set<string>();
          inputs.add(file);
          variants.set(format, inputs);
        }
      }
      if (variants.size < 2) continue;
      const items = [...variants.values()]
        .map((files) => ({
          subject: [...files]
            .map((f) => path.posix.relative(pkg.dir, f))
            .sort()
            .join(", "),
          ...sizesOnlyThrough(graph, roots, files, bytes),
        }))
        .sort(
          (a, b) => b.bytes - a.bytes || a.subject.localeCompare(b.subject),
        );
      const saved = sumSizes(items.slice(1));
      if (saved.bytes < DUPLICATE_MIN_EXTRA_BYTES) continue;
      result.push({
        kind: "alternate-package-implementations",
        level: "warn",
        title: `${pkg.label} bundles alternate implementations in ${b.id}, potentially adding ${formatSizes(saved)}`,
        detail: `The same installation at ${pkg.dir} includes matching ESM/CommonJS implementations: ${listItems(items)}. Standardize imports on one implementation where their APIs are compatible. Estimates include code and source maps only each implementation pulls in, including its helper files. Use \`npx @convex-dev/bundle-analyzer packages ${pkg.label}\` to see which files import it.`,
        ...savings(saved),
        buildIds: [b.id],
        subject: pkg.label,
        items,
      });
    }
  }
  return result;
}

function implementationVariant(
  file: string,
): { path: string; format: string } | null {
  const extension = /\.(mjs|cjs|js)$/.exec(file);
  if (!extension) return null;
  const directory = /(^|\/)(esm|cjs|commonjs)(?=\/)/.exec(file);
  const format = directory
    ? directory[2] === "esm"
      ? "esm"
      : "cjs"
    : extension[1];
  // Every format shares the marker so sibling directories group together.
  // Its position distinguishes root files from format subdirectories.
  return {
    path: file
      .replace(/(^|\/)(esm|cjs|commonjs)(?=\/)/, "$1{format}")
      .replace(/\.(mjs|cjs|js)$/, ".js"),
    format,
  };
}

/**
 * "use node" files are the Node build's entry points. In the same component's
 * Convex runtime build, a plain file importing one needs an importer fix;
 * a leaked entry point needs corrected entry point selection. Each leaked
 * file's size includes the code and source maps only it pulls in.
 */
function nodeFileInsights(
  builds: AnalyzedBuild[],
  graphs: BuildGraphs,
): Insight[] {
  const result: Insight[] = [];
  for (const node of builds) {
    if (node.kind !== "node") continue;
    const useNodeFiles = new Set(
      node.outputs.flatMap((o) => (o.entryPoint ? [o.entryPoint] : [])),
    );
    const nodeGraph = inputGraph(node);
    const importedByNodeCode = new Set([...nodeGraph.values()].flat());
    for (const b of builds) {
      if (
        b.kind !== "convex" ||
        b.component.definitionPath !== node.component.definitionPath
      ) {
        continue;
      }
      const { graph, roots, bytes } = graphs.get(b);
      const all = reachable(graph, roots);
      const leaked = new Set([...all].filter((p) => useNodeFiles.has(p)));
      if (leaked.size === 0) continue;
      const saved = sizesOnlyThrough(graph, roots, leaked, bytes);
      if (saved.codeBytes === 0) continue;

      // Imports from files without "use node" into "use node" files, and
      // which files without "use node" import each file.
      const importedBy = new Map<string, string[]>();
      const plainImporters = new Map<string, number>();
      for (const [from, imports] of graph) {
        if (!all.has(from) || useNodeFiles.has(from)) continue;
        for (const to of imports) {
          plainImporters.set(to, (plainImporters.get(to) ?? 0) + 1);
          if (leaked.has(to)) {
            importedBy.set(to, [...(importedBy.get(to) ?? []), from]);
          }
        }
      }
      for (const p of leaked) {
        if (roots.includes(p) && !importedBy.has(p)) importedBy.set(p, []);
      }
      // Each "use node" file with the files without "use node" that import
      // it, and the code fixing all those imports would remove.
      const items: InsightItem[] = [...importedBy.entries()]
        .map(([nodeFile, from]) => ({
          subject: nodeFile,
          entryPoint: roots.includes(nodeFile),
          ...sizesOnlyThrough(graph, roots, new Set([nodeFile]), bytes),
          importedBy: from.sort(),
          fixes: from.map((file) => ({
            file,
            // Only Node code imports it, so it can move to the node build.
            fix:
              !plainImporters.has(file) && importedByNodeCode.has(file)
                ? ("add-use-node" as const)
                : ("move" as const),
          })),
        }))
        .sort(
          (x, y) => y.bytes - x.bytes || x.subject.localeCompare(y.subject),
        );
      const fixText = (i: InsightItem) => {
        if (i.entryPoint)
          return `Check why ${i.subject} is an entry point in ${b.id}; it also appears in the Node build. Correct the entry point selection so its runtime matches its "use node" directive.${i.importedBy!.length > 0 ? ' Its Convex runtime importers must also move the code they use into a file without "use node".' : ""}`;
        const add = i.fixes!.filter((f) => f.fix === "add-use-node");
        const move = i.fixes!.filter((f) => f.fix === "move");
        const names = (list: typeof add) => fileList(list.map((f) => f.file));
        const addFix =
          add.length === i.fixes!.length
            ? `add "use node" to ${add.length === 1 ? "it" : "them"}. Only "use node" files import ${add.length === 1 ? "it" : "them"}, so this works if ${add.length === 1 ? "it defines" : "they define"} no queries or mutations.`
            : `add "use node" to ${names(add)}, which only "use node" files import`;
        const moveFix = `move what ${move.length === i.fixes!.length ? (move.length === 1 ? "it uses" : "they use") : `${names(move)} use${move.length === 1 ? "s" : ""}`} from ${i.subject} into a file without "use node".`;
        return `Fix: ${
          move.length === 0
            ? addFix
            : add.length === 0
              ? moveFix
              : `${addFix}, and ${moveFix}`
        }`;
      };
      const importText = (i: InsightItem) =>
        i.importedBy!.length > 0
          ? `${fileList(i.importedBy!)} import${i.importedBy!.length === 1 ? "s" : ""} "use node" file ${i.subject}`
          : `Entry ${i.subject} is bundled into ${b.id}`;
      const files = new Set(items.flatMap((i) => i.importedBy!));

      if (items.length === 1 && files.size === 0) {
        // A leaked file no other file imports is itself a Convex runtime entry.
        const entry = items[0].subject;
        result.push({
          kind: "use-node-file-in-convex-runtime",
          level: "warn",
          title: `${entry} has "use node" but is bundled into ${b.id}, adding ${formatSizes(saved)}`,
          detail: `It and the code only it imports are bundled for the Convex runtime too.`,
          ...savings(saved),
          buildIds: [b.id],
          subject: entry,
          items,
          rows: items.map((i) => ({
            ...sizes(i.codeBytes, i.sourceMapBytes),
            text: importText(i),
            note: fixText(i),
          })),
        });
        continue;
      }
      const [first] = items;
      const one = items.length === 1 && files.size === 1 && !first.entryPoint;
      result.push({
        kind: "use-node-file-in-convex-runtime",
        level: "warn",
        title: one
          ? `${importText(first)}, adding ${formatSizes(saved)} to ${b.id}`
          : items.some((i) => i.entryPoint)
            ? `${b.id} bundles ${plural(items.length, '"use node" file')}, adding ${formatSizes(saved)}`
            : `${plural(files.size, "file")} bring${files.size === 1 ? "s" : ""} "use node" code into ${b.id}, adding ${formatSizes(saved)}`,
        detail: one
          ? `It doesn't have "use node", so the "use node" code it imports is bundled for the Convex runtime too. ${fixText(first)}`
          : `Each item names a "use node" file bundled for the Convex runtime, its importers or entry point status, and the code and source maps only it pulls into ${b.id}:`,
        ...savings(saved),
        buildIds: [b.id],
        subject: one ? first.subject : null,
        items,
        ...(one
          ? {}
          : {
              rows: items.map((i) => ({
                ...sizes(i.codeBytes, i.sourceMapBytes),
                text: importText(i),
                note: fixText(i),
              })),
            }),
      });
    }
  }
  return result;
}

/**
 * Large packages in both the Convex runtime and Node bundles of a component.
 * The two are bundled separately, so a package both import is included in
 * each. Lists the files of yours that pull it into each runtime, so you can
 * tell which one needs it.
 */
function bothRuntimesInsights(
  builds: AnalyzedBuild[],
  graphs: BuildGraphs,
): Insight[] {
  const result: Insight[] = [];
  for (const node of builds) {
    if (node.kind !== "node") continue;
    const convex = builds.find(
      (b) =>
        b.kind === "convex" &&
        b.component.definitionPath === node.component.definitionPath,
    );
    if (!convex) continue;
    const useNodeFiles = new Set(
      node.outputs.flatMap((o) => (o.entryPoint ? [o.entryPoint] : [])),
    );
    const convexCode = npmSizes(convex);
    for (const [label, nodeBytes] of npmSizes(node)) {
      const convexBytes = convexCode.get(label) ?? sizes();
      if (
        nodeBytes.codeBytes === 0 ||
        convexBytes.codeBytes === 0 ||
        nodeBytes.bytes < BOTH_RUNTIMES_MIN_BYTES ||
        convexBytes.bytes < BOTH_RUNTIMES_MIN_BYTES
      ) {
        continue;
      }
      const sides = [
        { b: convex, bytes: convexBytes },
        { b: node, bytes: nodeBytes },
      ].map((side) => ({
        ...side,
        importers: targetUse(side.b, (p) => {
          const a = side.b.attribution.get(p);
          return a?.origin === "npm" && ownerLabel(a) === label;
        }).files,
      }));
      const idWidth = Math.max(...sides.map((side) => side.b.id.length));
      const saved =
        convexBytes.bytes <= nodeBytes.bytes ? convexBytes : nodeBytes;
      const { graph, roots } = graphs.get(convex);
      const throughPlainCode = reachable(graph, roots, useNodeFiles);
      const packageInputs = [...convex.attribution]
        .filter(([, a]) => a.origin === "npm" && ownerLabel(a) === label)
        .map(([p]) => p);
      if (
        packageInputs.length > 0 &&
        packageInputs.every((p) => !throughPlainCode.has(p))
      )
        continue;
      result.push({
        kind: "package-in-both-runtimes",
        level: "warn",
        title: `${label} is bundled into both runtimes, ${formatSizes(convexBytes)} in ${convex.id} and ${formatSizes(nodeBytes)} in ${node.id}`,
        detail: `The Convex runtime and Node are bundled separately, so a package used by both is included in each. If only one runtime needs it, removing its use from the other runtime's files saves up to ${formatSizes(saved)}. Files whose bundled code uses it in each runtime:`,
        ...savings(saved),
        buildIds: [convex.id, node.id],
        subject: label,
        items: sides.map((side) => ({
          subject: side.b.id,
          ...side.bytes,
          importedBy: [...side.importers.keys()],
        })),
        rows: sides.map((side) => {
          const files = [...side.importers.keys()];
          const via = [
            ...new Set([...side.importers.values()].flatMap((v) => [...v])),
          ].sort();
          return {
            ...side.bytes,
            text: `${side.b.id.padEnd(idWidth)}  ${files.length > 0 ? fileList(files) : "no file of yours"}${via.length > 0 ? ` via ${joinAnd(via)}` : ""}`,
          };
        }),
      });
    }
  }
  return result;
}

/**
 * Large packages only Node bundles include. Marking one external installs it
 * on the server instead of bundling it, which shrinks uploaded code but not
 * the deployment's size, so `savedBytes` is 0.
 */
function externalPackageInsights(
  builds: AnalyzedBuild[],
  projectRoot: string | undefined,
): Insight[] {
  const byPackage = new Map<
    string,
    { builds: Map<string, Sizes>; dirs: Set<string> }
  >();
  for (const node of builds) {
    if (node.kind !== "node") continue;
    const convex = builds.find(
      (b) =>
        b.kind === "convex" &&
        b.component.definitionPath === node.component.definitionPath,
    );
    const convexCode = convex ? npmSizes(convex) : new Map<string, Sizes>();
    const dirs = new Map<string, Set<string>>();
    for (const o of node.outputs) {
      for (const i of o.inputs) {
        if (i.origin !== "npm" || !i.packageDir) continue;
        const key = ownerLabel(i);
        dirs.set(key, (dirs.get(key) ?? new Set()).add(i.packageDir));
      }
    }
    for (const [label, bytes] of npmSizes(node)) {
      if (bytes.codeBytes === 0 || (convexCode.get(label)?.codeBytes ?? 0) > 0)
        continue;
      const entry = byPackage.get(label) ?? {
        builds: new Map(),
        dirs: new Set(),
      };
      entry.builds.set(node.id, bytes);
      for (const d of dirs.get(label) ?? []) entry.dirs.add(d);
      byPackage.set(label, entry);
    }
  }
  return [...byPackage.entries()]
    .filter(
      ([, entry]) =>
        sumSizes(entry.builds.values()).bytes >= EXTERNAL_PACKAGE_MIN_BYTES,
    )
    .map(([label, { builds: byBuild, dirs }]) => {
      const bytes = sumSizes(byBuild.values());
      const buildIds = [...byBuild.keys()];
      const installed =
        projectRoot === undefined
          ? 0
          : [...dirs].reduce(
              (n, d) => n + directorySize(path.resolve(projectRoot, d)),
              0,
            );
      return {
        kind: "external-package-candidate" as const,
        level: "info" as const,
        title: `${label} adds ${formatSizes(bytes)} to ${joinAnd(buildIds)}, and only "use node" code uses it`,
        detail: [
          `Adding ${packageName(label)} to \`node.externalPackages\` in convex.json installs it on the server instead of bundling it.`,
          `That may reduce the size of the code you upload, but may increase your overall bundle size, since the whole installed package counts rather than the code bundling keeps.`,
          ...(installed > 0
            ? [
                `Installed, it takes ${formatBytes(installed)} in your node_modules, not counting its dependencies.`,
              ]
            : []),
          `For more information, see: ${EXTERNAL_PACKAGES_DOCS}`,
        ].join(" "),
        ...savings(sizes()),
        buildIds,
        subject: label,
        items: [...byBuild]
          .map(([id, value]) => ({ subject: id, ...value }))
          .sort((a, b) => b.bytes - a.bytes),
      };
    });
}

/** `zod` from `zod@3.25.76`, `@scope/pkg` from `@scope/pkg@1.0.0`. */
function packageName(label: string): string {
  const at = label.lastIndexOf("@");
  return at > 0 ? label.slice(0, at) : label;
}

/** Attributed npm package sizes in a build, by name and version. */
function npmSizes(b: AnalyzedBuild): Map<string, Sizes> {
  return new Map(
    packageUsage([b])
      .filter((p) => p.origin === "npm")
      .map((p) => [p.key, sizes(p.codeBytes, p.sourceMapBytes)]),
  );
}

const TEST_FILE = /(^|\/)(__tests__\/|[^/]+\.(test|spec)\.[cm]?[jt]sx?$)/;
/** Paths that look like test helpers, mocks or fixtures. */
const TEST_LIKE =
  /(^|\/)((__tests__|__mocks__|tests?|mocks?|fixtures?|testing)\/|(tests?|mocks?|fixtures?)([-_.A-Z][^/]*)?\.[cm]?[jt]sx?$)/;

/**
 * Function modules are every file in convex/ except ones with more than one
 * dot in the name, so renaming a helper with a second dot keeps it out.
 */
function renameHint(files: string[]): string {
  const one = files.length === 1;
  const base = files[0].split("/").pop()!;
  const renamed = base.replace(/(\.[cm]?[jt]sx?)$/, ".helpers$1");
  return `${one ? "It's" : "They're"} deployed as ${one ? "a function module" : "function modules"} because ${one ? "it's" : "they're"} in convex/. The bundler skips files with more than one dot in the name, so renaming ${one ? "it" : "them"} (e.g. \`${base}\` → \`${renamed}\`) or moving ${one ? "it" : "them"} out of convex/ leaves ${one ? "it" : "them"} out.`;
}

/** devDependencies and test files that deployed functions import. */
function devCodeInsights(
  builds: AnalyzedBuild[],
  devDependencies: ReadonlySet<string>,
  graphs: BuildGraphs,
): Insight[] {
  const result: Insight[] = [];
  const deployed = builds.filter((b) => isFunctionRuntime(b.kind));
  // A test-like function module only test-like files import, which renaming
  // or moving would drop from the bundle.
  const isTestEntry = (b: AnalyzedBuild, p: string) => {
    const { roots, doms } = graphs.get(b);
    return (
      (TEST_LIKE.test(p) || TEST_FILE.test(p)) &&
      roots.includes(p) &&
      (doms.importers.get(p) ?? []).every(
        (i) => TEST_LIKE.test(i) || TEST_FILE.test(i),
      )
    );
  };
  for (const pkg of packageUsage(deployed)) {
    if (pkg.origin !== "npm" || !devDependencies.has(pkg.name)) continue;
    if (pkg.codeBytes === 0 || pkg.bytes < DEV_DEPENDENCY_MIN_BYTES) continue;
    const buildIds = [...pkg.builds.keys()];
    const appImporters = new Set<string>();
    const packageImporters = new Set<string>();
    let onlyTestEntries = true;
    let saved = sizes();
    for (const b of deployed) {
      if (!pkg.builds.has(b.id)) continue;
      const { graph, roots, bytes } = graphs.get(b);
      const appInBuild = new Set<string>();
      const isPkg = (p: string) => {
        const a = b.attribution.get(p);
        return !!a && a.origin === "npm" && ownerLabel(a) === pkg.key;
      };
      for (const [from, imports] of graph) {
        if (isPkg(from) || !imports.some(isPkg)) continue;
        const a = b.attribution.get(from);
        if (a?.origin === "npm" || a?.origin === "convex") {
          packageImporters.add(ownerLabel(a));
        } else {
          appImporters.add(from);
          appInBuild.add(from);
          if (!isTestEntry(b, from)) onlyTestEntries = false;
        }
      }
      // Removing app imports leaves production packages' imports intact.
      const withoutAppImports = new Map(
        [...graph].map(([from, imports]) => [
          from,
          appInBuild.has(from) ? imports.filter((to) => !isPkg(to)) : imports,
        ]),
      );
      const before = reachable(graph, roots);
      const after = reachable(withoutAppImports, roots);
      saved = sumSizes([
        saved,
        sumSizes(
          [...before]
            .filter((p) => isPkg(p) && !after.has(p))
            .map((p) => bytes.get(p) ?? sizes()),
        ),
      ]);
    }
    const app = [...appImporters].sort();
    const packages = [...packageImporters].sort();
    const s = app.length === 1 ? "s" : "";
    if (app.length === 0 && packages.length > 0) {
      result.push({
        kind: "dev-dependency-bundled",
        level: "info",
        title: `${pkg.key} is a devDependency, but ${joinAnd(packages)} need${packages.length === 1 ? "s" : ""} it at runtime`,
        detail: `It's bundled into ${joinAnd(buildIds)} (${formatSizes(pkg)}) because ${packages.length === 1 ? "that package imports" : "those packages import"} it, not your code. Listing it under dependencies records that it's needed at runtime.`,
        ...savings(sizes()),
        buildIds,
        subject: pkg.key,
        items: [],
      });
      continue;
    }
    result.push({
      kind: "dev-dependency-bundled",
      level: "info",
      title: `${pkg.key}, a devDependency, is bundled into ${joinAnd(buildIds)}, adding ${formatSizes(pkg)}`,
      detail:
        onlyTestEntries && app.length > 0 && packages.length === 0
          ? `Only ${fileList(app, 3)} import${s} it. ${renameHint(app)}`
          : `${fileList(app, 3)} import${s} it${packages.length > 0 ? `, and so do ${joinAnd(packages)}` : ""}. If only tests or scripts need it, refactor it out of those files. Use \`npx @convex-dev/bundle-analyzer packages ${pkg.name}\` to see each import chain.`,
      ...savings(saved),
      buildIds,
      subject: pkg.key,
      items: [],
    });
  }
  const testFiles = new Map<
    string,
    { sizes: Sizes; buildIds: string[]; entry: boolean }
  >();
  for (const b of deployed) {
    const { doms } = graphs.get(b);
    for (const p of doms.idom.keys()) {
      if (b.attribution.get(p)?.origin !== "app" || !TEST_FILE.test(p)) {
        continue;
      }
      const bytes = doms.dominatedSizes.get(p) ?? sizes();
      if (bytes.codeBytes === 0) continue;
      const entry = testFiles.get(p) ?? {
        sizes: sizes(),
        buildIds: [],
        entry: true,
      };
      entry.sizes = sumSizes([entry.sizes, bytes]);
      entry.buildIds.push(b.id);
      if (!isTestEntry(b, p)) entry.entry = false;
      testFiles.set(p, entry);
    }
  }
  for (const [path, { sizes: bytes, buildIds, entry }] of testFiles) {
    result.push({
      kind: "test-file-bundled",
      level: "info",
      title: `Test file ${path} is bundled into ${joinAnd(buildIds)}, adding ${formatSizes(bytes)}`,
      detail: entry
        ? `${renameHint([path])} That also leaves out the code only it imports.`
        : `A deployed function imports it, along with the code only it imports. Use \`npx @convex-dev/bundle-analyzer files ${path}\` to see what imports it.`,
      ...savings(bytes),
      buildIds,
      subject: path,
      items: [],
    });
  }
  return result;
}

/**
 * Packages bundled into many components. Each component is bundled on its
 * own, so the copies beyond the largest are what sharing it would save.
 */
function sharedPackageInsights(builds: AnalyzedBuild[]): Insight[] {
  const byId = new Map(builds.map((b) => [b.id, b]));
  const result: Insight[] = [];
  for (const pkg of packageUsage(builds)) {
    if (pkg.origin !== "npm") continue;
    const byComponent = new Map<string, Sizes>();
    for (const [id, n] of pkg.builds) {
      if (n.codeBytes === 0) continue;
      const label = byId.get(id)!.component.label;
      byComponent.set(label, sumSizes([byComponent.get(label) ?? sizes(), n]));
    }
    if (
      byComponent.size < SHARED_PACKAGE_MIN_COMPONENTS ||
      pkg.bytes < SHARED_PACKAGE_MIN_BYTES
    ) {
      continue;
    }
    const items = [...byComponent.entries()]
      .map(([subject, value]) => ({ subject, ...value }))
      .sort((x, y) => y.bytes - x.bytes || x.subject.localeCompare(y.subject));
    result.push({
      kind: "package-in-many-components",
      level: "info",
      title: `${pkg.key} is bundled separately into ${byComponent.size} components, ${formatSizes(pkg)} in total`,
      detail: `Each component is bundled on its own, so a dependency they share is included once per component: ${listItems(items)}.`,
      ...savings(
        sizes(
          pkg.codeBytes - items[0].codeBytes,
          pkg.sourceMapBytes - items[0].sourceMapBytes,
        ),
      ),
      buildIds: [...pkg.builds]
        .filter(([, s]) => s.codeBytes > 0)
        .map(([id]) => id),
      subject: pkg.key,
      items,
    });
  }
  return result;
}

export type Duplicate = {
  name: string;
  /** Distinct installations (package dirs) of the package in a single build. */
  copies: ({ version: string | null; packageDir: string } & Sizes)[];
  buildId: string;
};

export function duplicates(builds: AnalyzedBuild[]): Duplicate[] {
  const result: Duplicate[] = [];
  for (const b of builds) {
    const byName = new Map<
      string,
      Map<string, { version: string | null } & Sizes>
    >();
    for (const o of b.outputs) {
      for (const i of o.inputs) {
        if (!i.packageName || !i.packageDir) continue;
        let dirs = byName.get(i.packageName);
        if (!dirs) {
          dirs = new Map();
          byName.set(i.packageName, dirs);
        }
        const entry = dirs.get(i.packageDir) ?? {
          version: i.packageVersion,
          ...sizes(),
        };
        Object.assign(
          entry,
          sumSizes([entry, sizes(i.bytesInOutput, i.sourceMapBytes)]),
        );
        dirs.set(i.packageDir, entry);
      }
    }
    for (const [name, dirs] of byName) {
      const copies = [...dirs.entries()].filter(
        ([, value]) => value.codeBytes > 0,
      );
      if (copies.length < 2) continue;
      result.push({
        name,
        buildId: b.id,
        copies: copies
          .map(([packageDir, v]) => ({ packageDir, ...v }))
          .sort((x, y) => y.bytes - x.bytes),
      });
    }
  }
  return result;
}

function formatSizes(value: Sizes): string {
  return `${formatBytes(value.codeBytes)} of code and ${formatBytes(value.sourceMapBytes)} of source maps`;
}

function savings(value: Sizes) {
  return {
    savedBytes: value.bytes,
    savedCodeBytes: value.codeBytes,
    savedSourceMapBytes: value.sourceMapBytes,
  };
}
