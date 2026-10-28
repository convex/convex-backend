import stringWidth from "string-width";
import {
  AnalyzedBuild,
  InputAttribution,
  displayPath,
  inputBytes,
  ownerLabel,
} from "./model.js";
import { targetUse } from "./usage.js";
import {
  CODE_COLOR,
  bold,
  colored,
  dim,
  formatBytes,
  heading,
  plural,
  truncate,
} from "./format.js";

export type ImportStep = {
  path: string;
  /** Human-readable location, e.g. `zod@3.25.76/index.js` or `convex/messages.ts`. */
  display: string;
  specifier: string | null;
};

export type ImportChain = {
  buildId: string;
  entryPoint: string;
  target: string;
  /** The package copy (`name@version`) or file the chain ends at. */
  targetGroup: string;
  chain: ImportStep[];
};

/**
 * What to find chains to: every copy of a package, by name (`zod`) or by name
 * and version (`zod@3.25.76`), or specific files by metafile input path.
 */
export type ChainQuery =
  | { kind: "package"; name: string }
  | { kind: "files"; inputPaths: ReadonlySet<string> };

export function matchesPackage(
  attribution: InputAttribution | undefined,
  name: string,
): attribution is InputAttribution {
  return (
    !!attribution?.packageName &&
    (attribution.packageName === name || ownerLabel(attribution) === name)
  );
}

/** Shortest import chain from each entry point to each package copy or file. */
export function importChains(
  builds: AnalyzedBuild[],
  query: ChainQuery,
): ImportChain[] {
  const results: ImportChain[] = [];
  const isPackageQuery = query.kind === "package";
  for (const b of builds) {
    const groups = new Map<string, Set<string>>();
    for (const inputPath of Object.keys(b.metafile.inputs)) {
      const attribution = b.attribution.get(inputPath);
      let group: string;
      if (query.kind === "package") {
        if (!matchesPackage(attribution, query.name)) continue;
        // Distinct install directories are distinct copies, even at the same version.
        group = `${ownerLabel(attribution)}\0${attribution.packageDir ?? ""}`;
      } else {
        if (!query.inputPaths.has(inputPath)) continue;
        group = inputPath;
      }
      groups.set(group, (groups.get(group) ?? new Set()).add(inputPath));
    }
    if (groups.size === 0) continue;
    const searches = b.outputs.flatMap((o) =>
      o.entryPoint
        ? [{ entryPoint: o.entryPoint, ...search(b, o.entryPoint) }]
        : [],
    );
    for (const [group, targets] of groups) {
      for (const { entryPoint, prev, order } of searches) {
        const chain = shortestChain(prev, order, targets);
        if (!chain) continue;
        const target = chain[chain.length - 1].path;
        results.push({
          buildId: b.id,
          entryPoint,
          target,
          targetGroup: isPackageQuery
            ? group.split("\0")[0]
            : displayPath(target, b.attribution.get(target)),
          chain: chain.map((step) => ({
            ...step,
            display: displayPath(step.path, b.attribution.get(step.path)),
          })),
        });
      }
    }
  }
  return results.sort((a, b) => a.chain.length - b.chain.length);
}

type Predecessors = Map<
  string,
  { from: string; specifier: string | null } | null
>;

/**
 * Breadth-first search over the whole import graph reachable from `from`, so
 * one search per entry point serves every query target.
 */
function search(
  b: AnalyzedBuild,
  from: string,
): { prev: Predecessors; order: Map<string, number> } {
  const prev: Predecessors = new Map([[from, null]]);
  const order = new Map<string, number>();
  const queue = [from];
  for (let head = 0; head < queue.length; head++) {
    const current = queue[head];
    order.set(current, head);
    // esbuild's metafile is a plain object keyed by path.
    const imports = Object.hasOwn(b.metafile.inputs, current)
      ? b.metafile.inputs[current].imports
      : [];
    for (const imp of imports) {
      if (imp.external || prev.has(imp.path)) continue;
      prev.set(imp.path, { from: current, specifier: imp.original ?? null });
      queue.push(imp.path);
    }
  }
  return { prev, order };
}

/** The chain to whichever of `targets` the search reached first. */
function shortestChain(
  prev: Predecessors,
  order: Map<string, number>,
  targets: Set<string>,
): { path: string; specifier: string | null }[] | null {
  let nearest: string | undefined;
  for (const t of targets) {
    const o = order.get(t);
    if (o !== undefined && (nearest === undefined || o < order.get(nearest)!)) {
      nearest = t;
    }
  }
  if (nearest === undefined) return null;
  const chain: { path: string; specifier: string | null }[] = [];
  let node: string | undefined = nearest;
  while (node !== undefined) {
    const p = prev.get(node);
    chain.push({ path: node, specifier: p?.specifier ?? null });
    node = p?.from;
  }
  return chain.reverse();
}

/** Every entry point's chain to one package copy or file in one build. */
export type ChainTarget = {
  buildId: string;
  /** `name@version` for a package copy, the file's display path otherwise. */
  label: string;
  /** Install directory of a package copy, relative to the project root. */
  packageDir: string | null;
  /** The matched file's metafile input path, for a path match. */
  inputPath: string | null;
  /** Code plus source map bytes the copy or file adds to the build. */
  bytes: number;
  codeBytes: number;
  sourceMapBytes: number;
  /** Shortest first. */
  chains: ImportChain[];
  /**
   * What imports the copy or file itself: a package (`name@version`) or one
   * of your files. Entry points can reach it without importing it directly.
   */
  directImporters: string[];
  /** Entry points that reach it through imports. */
  reachedBy: number;
  /**
   * Entry points whose code includes its code. esbuild splits chunks after
   * tree-shaking, so these are the ones that use what they import. `chains`
   * only runs from these, unless there are none.
   */
  usedBy: string[];
  /** Your files whose imports keep its code; see `targetUse`. */
  usedThrough: string[];
};

type Sizes = Pick<ChainTarget, "bytes" | "codeBytes" | "sourceMapBytes">;

/**
 * The chains to each package copy or file in each build, with what the copy
 * or file adds to that build. Ordered by build, then by size.
 */
export function chainTargets(
  builds: AnalyzedBuild[],
  query: ChainQuery,
): ChainTarget[] {
  const results = importChains(builds, query);
  const byId = new Map(builds.map((b) => [b.id, b]));
  const isPackageMatch = query.kind === "package";

  // Each target's sizes, by package copy (label and directory) for a package
  // match and by input path otherwise, summed in one pass over each build's
  // inputs. Versions without a known directory share a null `packageDir`, so
  // the label keeps them apart.
  const copyKey = (label: string, packageDir: string | null) =>
    `${label}\0${packageDir ?? ""}`;
  const sizesByBuild = new Map<string, Map<string, Sizes>>();
  const sizesFor = (b: AnalyzedBuild) => {
    let sizes = sizesByBuild.get(b.id);
    if (sizes === undefined) {
      sizes = new Map();
      for (const o of b.outputs) {
        for (const i of o.inputs) {
          if (query.kind === "package" && !matchesPackage(i, query.name)) {
            continue;
          }
          const key = isPackageMatch
            ? copyKey(ownerLabel(i), i.packageDir)
            : i.path;
          const s = sizes.get(key) ?? {
            bytes: 0,
            codeBytes: 0,
            sourceMapBytes: 0,
          };
          s.bytes += inputBytes(i);
          s.codeBytes += i.bytesInOutput;
          s.sourceMapBytes += i.sourceMapBytes;
          sizes.set(key, s);
        }
      }
      sizesByBuild.set(b.id, sizes);
    }
    return sizes;
  };

  // Each build's importers of each input, as display labels.
  const importersByBuild = new Map<string, Map<string, Set<string>>>();
  const importersIn = (b: AnalyzedBuild) => {
    let importers = importersByBuild.get(b.id);
    if (importers === undefined) {
      importers = new Map();
      for (const [from, input] of Object.entries(b.metafile.inputs)) {
        const a = b.attribution.get(from);
        const label =
          a?.packageName && (a.origin === "npm" || a.origin === "convex")
            ? ownerLabel(a)
            : displayPath(from, a);
        for (const imp of input.imports) {
          if (imp.external) continue;
          let set = importers.get(imp.path);
          if (!set) importers.set(imp.path, (set = new Set()));
          set.add(`${label}\0${from}`);
        }
      }
      importersByBuild.set(b.id, importers);
    }
    return importers;
  };
  const directImporters = (b: AnalyzedBuild, files: Set<string>) => {
    const importers = importersIn(b);
    const labels = new Set<string>();
    for (const f of files) {
      for (const entry of importers.get(f) ?? []) {
        const [label, from] = entry.split("\0");
        if (!files.has(from)) labels.add(label);
      }
    }
    return [...labels].sort();
  };

  const targets = new Map<string, ChainTarget>();
  for (const r of results) {
    const b = byId.get(r.buildId)!;
    const attribution = b.attribution.get(r.target);
    const packageDir = isPackageMatch
      ? (attribution?.packageDir ?? null)
      : null;
    const inputPath = isPackageMatch ? null : r.target;
    const key = [r.buildId, r.targetGroup, packageDir ?? inputPath].join("\0");
    let target = targets.get(key);
    if (target === undefined) {
      const sizes = sizesFor(b).get(
        isPackageMatch ? copyKey(r.targetGroup, packageDir) : r.target,
      );
      const files = isPackageMatch
        ? new Set(
            Object.keys(b.metafile.inputs).filter((p) => {
              const a = b.attribution.get(p);
              return (
                matchesPackage(a, r.targetGroup) &&
                (a.packageDir ?? null) === packageDir
              );
            }),
          )
        : new Set([r.target]);
      const use = targetUse(b, (p) => files.has(p));
      target = {
        buildId: r.buildId,
        label: r.targetGroup,
        packageDir,
        inputPath,
        bytes: sizes?.bytes ?? 0,
        codeBytes: sizes?.codeBytes ?? 0,
        sourceMapBytes: sizes?.sourceMapBytes ?? 0,
        chains: [],
        directImporters: directImporters(b, files),
        reachedBy: 0,
        usedBy: use.usedBy,
        usedThrough: [...use.files.keys()],
      };
      targets.set(key, target);
    }
    target.reachedBy += 1;
    // Chains from modules tree-shaking leaves it out of mislead, unless no
    // module keeps any of its code.
    if (target.usedBy.length === 0 || target.usedBy.includes(r.entryPoint)) {
      target.chains.push(r);
    }
  }

  const buildOrder = new Map(builds.map((b, idx) => [b.id, idx]));
  return [...targets.values()].sort(
    (a, b) =>
      buildOrder.get(a.buildId)! - buildOrder.get(b.buildId)! ||
      b.bytes - a.bytes ||
      a.label.localeCompare(b.label),
  );
}

export function chainTargetsJson(targets: ChainTarget[], limit: number) {
  return targets.map((t) => ({
    build: t.buildId,
    target: t.label,
    packageDir: t.packageDir,
    path: t.inputPath,
    bytes: t.bytes,
    codeBytes: t.codeBytes,
    sourceMapBytes: t.sourceMapBytes,
    entryPoints: t.reachedBy,
    usedBy: t.usedBy,
    usedThrough: t.usedThrough,
    directImporters: t.directImporters,
    chains: t.chains.slice(0, limit).map((c) => ({
      entryPoint: c.entryPoint,
      steps: c.chain.map((s) => ({
        path: s.path,
        display: s.display,
        specifier: s.specifier,
      })),
    })),
  }));
}

const TARGET_COLOR = "#ef6f6c";

/**
 * Each build's chains, under an "Imported by" heading. With `showTargets`,
 * each package copy or file gets a line with its size, so several copies in
 * one build stay apart.
 */
export function importedBySection(
  targets: ChainTarget[],
  options: { chains: number; width: number; showTargets: boolean },
): string[] {
  const { chains, width, showTargets } = options;
  const fit = (line: string) => truncate(line, width, "middle");
  const out = [bold("Imported by")];
  if (targets.length === 0) {
    out.push(dim("  No entry point imports it."));
    return out;
  }
  const buildIds = [...new Set(targets.map((t) => t.buildId))];
  for (const buildId of buildIds) {
    out.push(heading(buildId).trimEnd());
    const inBuild = targets.filter((t) => t.buildId === buildId);
    for (const t of inBuild) {
      if (showTargets) {
        out.push("", targetLine(t, width));
        // Copies at the same version, or files with the same display path,
        // are told apart by where they're installed.
        if (inBuild.filter((other) => other.label === t.label).length > 1) {
          out.push(fit(dim(`    installed at ${t.packageDir ?? t.inputPath}`)));
        }
        const unused = t.reachedBy - t.usedBy.length;
        if (t.usedBy.length > 0 && unused > 0) {
          out.push(
            fit(
              dim(
                `    ${plural(unused, "more entry point")} import${unused === 1 ? "s" : ""} it without using its code`,
              ),
            ),
          );
        }
        if (t.usedBy.length > 0 && t.usedThrough.length > 0) {
          const shown = t.usedThrough.slice(0, 3);
          const more = t.usedThrough.length - shown.length;
          out.push(
            fit(
              dim(
                `    used by your files ${shown.join(", ")}${more > 0 ? ` and ${more} more` : ""}`,
              ),
            ),
          );
        }
        if (t.directImporters.length > 0) {
          const shown = t.directImporters.slice(0, 3);
          const more = t.directImporters.length - shown.length;
          out.push(
            fit(
              dim(
                `    imported directly by ${shown.join(", ")}${more > 0 ? ` and ${more} more` : ""}`,
              ),
            ),
          );
        }
      }
      if (!showTargets) {
        out.push(
          fit(
            dim(
              `    reached from ${plural(t.reachedBy, "entry point")}; used by ${plural(t.usedBy.length, "entry point")}`,
            ),
          ),
        );
      }
      for (const c of t.chains.slice(0, chains)) {
        out.push(...chainLines(c.chain).map(fit));
      }
      if (t.chains.length > chains) {
        const hidden = t.chains.length - chains;
        out.push(
          fit(
            dim(
              `    … ${plural(hidden, "more entry point")} (use --limit ${t.chains.length} to show all)`,
            ),
          ),
        );
      }
    }
  }
  return out;
}

const MIN_LABEL_WIDTH = 12;

/** Truncates only the label, so the size stays visible at narrow widths. */
function targetLine(t: ChainTarget, width: number): string {
  const prefix = `  ${colored(TARGET_COLOR, "◆")} `;
  const size = `  ${formatBytes(t.bytes)}`;
  const from = `  ${dim(
    t.usedBy.length > 0
      ? `used by ${plural(t.usedBy.length, "entry point")}`
      : `reached from ${plural(t.reachedBy, "entry point")}`,
  )}`;
  const fixed = stringWidth(prefix + size);
  const suffix =
    width - fixed - stringWidth(from) >= MIN_LABEL_WIDTH ? size + from : size;
  const labelWidth = Math.max(
    MIN_LABEL_WIDTH,
    width - stringWidth(prefix + suffix),
  );
  return truncate(
    prefix + bold(truncate(t.label, labelWidth, "middle")) + suffix,
    width,
  );
}

function chainLines(chain: ImportStep[]): string[] {
  return chain.map((step, idx) => {
    const name =
      idx === chain.length - 1
        ? colored(TARGET_COLOR, step.display)
        : idx === 0
          ? colored(CODE_COLOR, step.display)
          : step.display;
    const specifier = step.specifier
      ? dim(`  import ${JSON.stringify(step.specifier)}`)
      : "";
    const lead = idx === 0 ? "    " : "    ".repeat(idx) + dim("└─▶ ");
    return `${lead}${name}${specifier}`;
  });
}
