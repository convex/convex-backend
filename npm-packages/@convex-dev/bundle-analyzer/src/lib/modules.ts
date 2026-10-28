import {
  AnalyzedBuild,
  AnalyzedOutput,
  BuildKind,
  CODE_GLUE_NAME,
  SourceOrigin,
  displayPath,
  ownerLabel,
} from "./model.js";
import {
  CODE_COLOR,
  barCells,
  dim,
  formatBytes,
  stackedBar,
  table,
  terminalWidth,
} from "./format.js";

export const SHARED_CHUNK_COLOR = "#6f7680";

/** The runtimes whose functions run from entry modules, in display order. */
export const FUNCTION_RUNTIMES = ["convex", "node"] as const;
export type FunctionRuntime = (typeof FUNCTION_RUNTIMES)[number];

/**
 * The heaviest modules by load size, optionally only one runtime's. With
 * `grouped`, modules that load the same shared chunks share a row, showing
 * the heaviest of them.
 */
export function modulesTable(
  builds: AnalyzedBuild[],
  limit: number,
  width = terminalWidth(),
  options: { runtime?: FunctionRuntime | undefined; grouped?: boolean } = {},
): string {
  const { runtime, grouped = false } = options;
  const loads = moduleLoads(builds).filter(
    (m) => runtime === undefined || m.kind === runtime,
  );
  const all = grouped ? groupBySharedChunks(loads) : loads.map((m) => [m]);
  const max = Math.max(1, ...loads.map((m) => m.loadBytes));
  const cells = barCells(width, 70, 16);
  const rows = all.slice(0, limit).map(([m, ...rest]) => [
    rest.length > 0 ? `${m.path} ${dim(`+${rest.length} more`)}` : m.path,
    dim(m.buildId),
    formatBytes(m.ownBytes),
    formatBytes(m.loadBytes - m.ownBytes),
    stackedBar(
      [
        { value: m.ownBytes, color: CODE_COLOR },
        { value: m.loadBytes - m.ownBytes, color: SHARED_CHUNK_COLOR },
      ],
      Math.max(1, Math.round((m.loadBytes / max) * cells)),
    ),
    String(m.chunks.length),
  ]);
  return table(
    [
      { header: "Module", flexible: true },
      { header: "Build", flexible: true },
      { header: "Own", align: "right" },
      { header: "Imports", align: "right" },
      { header: "", optional: true },
      { header: "Chunks", align: "right", optional: true },
    ],
    rows,
    width,
  );
}

/**
 * `loads` grouped by build and the exact set of shared chunks they load, in
 * the order of each group's heaviest module. Modules without chunks are
 * alone in their group.
 */
export function groupBySharedChunks(loads: ModuleLoad[]): ModuleLoad[][] {
  const groups = new Map<string, ModuleLoad[]>();
  for (const m of loads) {
    const key =
      m.chunks.length > 0
        ? [m.buildId, ...[...m.chunks].sort()].join("\0")
        : `${m.buildId}\0${m.path}\0own`;
    groups.set(key, [...(groups.get(key) ?? []), m]);
  }
  return [...groups.values()];
}

export type ModuleLoad = {
  buildId: string;
  kind: BuildKind;
  path: string;
  /** Code bytes in the entry module alone. */
  ownBytes: number;
  /** Own bytes plus every chunk it transitively imports. */
  loadBytes: number;
  /** Additional bytes only loaded if dynamic `import()`s run. */
  lazyBytes: number;
  chunks: string[];
};

/**
 * What the runtime must load and evaluate to run a function in each entry
 * module: the entry itself plus transitively imported shared chunks. The
 * Convex runtime compiles and evaluates exactly this closure on every call
 * (`ExecutionScope::register_module` follows static imports only). Schema and
 * auth config builds are left out since they only run at push time.
 */
export function moduleLoads(builds: AnalyzedBuild[]): ModuleLoad[] {
  const result: ModuleLoad[] = [];
  for (const b of builds) {
    if (!isFunctionRuntime(b.kind)) continue;
    const byPath = new Map(b.outputs.map((o) => [o.path, o]));
    for (const entry of b.outputs) {
      if (!entry.entryPoint) continue;
      const seen = closure(byPath, [entry.path], false);
      const lazy = closure(byPath, [entry.path], true);
      for (const p of seen) lazy.delete(p);
      const loaded = [...seen]
        .map((p) => byPath.get(p))
        .filter((o): o is AnalyzedOutput => !!o);
      result.push({
        buildId: b.id,
        kind: b.kind,
        path: entry.path,
        ownBytes: entry.codeBytes,
        loadBytes: loaded.reduce((n, o) => n + o.codeBytes, 0),
        lazyBytes: [...lazy].reduce(
          (n, p) => n + (byPath.get(p)?.codeBytes ?? 0),
          0,
        ),
        chunks: [...seen].filter((p) => p !== entry.path),
      });
    }
  }
  return result.sort((a, b) => b.loadBytes - a.loadBytes);
}

export function isFunctionRuntime(kind: BuildKind): kind is FunctionRuntime {
  return (FUNCTION_RUNTIMES as readonly BuildKind[]).includes(kind);
}

/**
 * A function module is an entry point. Chunks are code esbuild split out of
 * function modules: shared ones are loaded on some function module's every
 * call, lazy ones only through dynamic `import()`.
 */
export type ModuleRole = "function module" | "shared chunk" | "lazy chunk";

export type ModuleFile = {
  /** Metafile input path. */
  path: string;
  /** The path with its package, e.g. `zod@3.25.76/index.js`. */
  display: string;
  owner: string;
  /** For coloring by owner, as in `files`. */
  origin: SourceOrigin;
  packageName: string | null;
  bytes: number;
};

export type ImportedModule = {
  path: string;
  codeBytes: number;
  /** Code bytes by owner (package, `your code`, ...), largest first. */
  owners: { owner: string; bytes: number }[];
};

export type ModuleLoader = {
  path: string;
  /**
   * Code bytes the function module loads when it uses this module: its own
   * code and static imports, plus, for a lazy importer, this module and any
   * of its static imports the importer doesn't already load.
   */
  totalBytes: number;
};

export type ModuleDetail = {
  buildId: string;
  kind: FunctionRuntime;
  role: ModuleRole;
  path: string;
  codeBytes: number;
  /** Its own code plus every module it statically imports. */
  loadBytes: number;
  /** Each input file's code in the module, largest first. */
  files: ModuleFile[];
  /** Modules it statically imports, directly or indirectly, largest first. */
  imports: ImportedModule[];
  /** Modules loaded only if a dynamic `import()` runs, largest first. */
  lazy: ImportedModule[];
  /** Function modules that load it on every call, largest load first. */
  loadedBy: ModuleLoader[];
  /** Function modules that load it only through dynamic `import()`. */
  lazilyLoadedBy: ModuleLoader[];
  /**
   * For a function module whose source file esbuild moved into a shared
   * chunk, because other modules import that file too: the file and the chunk.
   */
  hoisted: { source: string; chunk: string } | null;
};

/**
 * Modules matching `query`, by module path (`messages.js`,
 * `_deps/3565P6XD.js`) or, for function modules, by source path
 * (`convex/messages.ts`), with or without the extension. An exact match wins;
 * otherwise `query` matches part of a path.
 */
export function lookupModules(
  builds: AnalyzedBuild[],
  query: string,
): ModuleDetail[] {
  const outputs = builds.flatMap((b) =>
    isFunctionRuntime(b.kind) ? b.outputs.map((o) => ({ b, o })) : [],
  );
  const names = (o: AnalyzedOutput) =>
    [o.path, o.entryPoint]
      .filter((n): n is string => n !== null)
      .flatMap((n) => [n, withoutExtension(n)]);
  const bare = withoutExtension(query);
  const exact = outputs.filter(({ o }) =>
    names(o).some((n) => n === query || n === bare),
  );
  const matches =
    exact.length > 0
      ? exact
      : outputs.filter(({ o }) => names(o).some((n) => n.includes(query)));
  return matches.map(({ b, o }) => moduleDetail(b, o));
}

function withoutExtension(path: string): string {
  return path.replace(/\.[cm]?[jt]sx?$/, "");
}

function moduleDetail(b: AnalyzedBuild, module: AnalyzedOutput): ModuleDetail {
  const byPath = new Map(b.outputs.map((o) => [o.path, o]));
  const loaded = closure(byPath, [module.path], false);
  const lazy = closure(byPath, [module.path], true);
  for (const p of loaded) lazy.delete(p);
  const imported = (paths: Iterable<string>) =>
    [...paths]
      .filter((p) => p !== module.path)
      .map((p) => byPath.get(p))
      .filter((o): o is AnalyzedOutput => !!o)
      .map(importedModule)
      .sort((x, y) => y.codeBytes - x.codeBytes);
  const imports = imported(loaded);

  const codeBytes = (paths: Iterable<string>) =>
    [...paths].reduce((n, p) => n + (byPath.get(p)?.codeBytes ?? 0), 0);
  const loadedBy: ModuleLoader[] = [];
  const lazilyLoadedBy: ModuleLoader[] = [];
  for (const entry of b.outputs) {
    if (!entry.entryPoint || entry.path === module.path) continue;
    const entryLoads = closure(byPath, [entry.path], false);
    if (entryLoads.has(module.path)) {
      loadedBy.push({ path: entry.path, totalBytes: codeBytes(entryLoads) });
    } else if (closure(byPath, [entry.path], true).has(module.path)) {
      lazilyLoadedBy.push({
        path: entry.path,
        totalBytes: codeBytes(new Set([...entryLoads, ...loaded])),
      });
    }
  }
  const largestLoadFirst = (x: ModuleLoader, y: ModuleLoader) =>
    y.totalBytes - x.totalBytes || x.path.localeCompare(y.path);

  const files: ModuleFile[] = module.inputs.map((i) => ({
    path: i.path,
    display: displayPath(i.path, i),
    owner: ownerLabel(i),
    origin: i.origin,
    packageName: i.packageName,
    bytes: i.bytesInOutput,
  }));
  if (module.unattributedBytes > 0) {
    files.push({
      path: CODE_GLUE_NAME,
      display: CODE_GLUE_NAME,
      owner: CODE_GLUE_NAME,
      origin: "virtual",
      packageName: null,
      bytes: module.unattributedBytes,
    });
  }

  return {
    buildId: b.id,
    kind: b.kind as FunctionRuntime,
    role: module.entryPoint
      ? "function module"
      : loadedBy.length > 0
        ? "shared chunk"
        : "lazy chunk",
    path: module.path,
    codeBytes: module.codeBytes,
    loadBytes: module.codeBytes + imports.reduce((n, c) => n + c.codeBytes, 0),
    files: files.sort((x, y) => y.bytes - x.bytes),
    imports,
    lazy: imported(lazy),
    loadedBy: loadedBy.sort(largestLoadFirst),
    lazilyLoadedBy: lazilyLoadedBy.sort(largestLoadFirst),
    hoisted: hoisted(b, module, loaded),
  };
}

function hoisted(
  b: AnalyzedBuild,
  module: AnalyzedOutput,
  loaded: Set<string>,
): ModuleDetail["hoisted"] {
  const source = module.entryPoint;
  const contains = (o: AnalyzedOutput) =>
    o.inputs.some((i) => i.path === source && i.bytesInOutput > 0);
  if (source === null || contains(module)) return null;
  const chunk = b.outputs.find(
    (o) => o !== module && loaded.has(o.path) && contains(o),
  );
  return chunk ? { source, chunk: chunk.path } : null;
}

function importedModule(o: AnalyzedOutput): ImportedModule {
  const owners = new Map<string, number>();
  for (const i of o.inputs) {
    const k = ownerLabel(i);
    owners.set(k, (owners.get(k) ?? 0) + i.bytesInOutput);
  }
  if (o.unattributedBytes > 0) {
    owners.set(
      CODE_GLUE_NAME,
      (owners.get(CODE_GLUE_NAME) ?? 0) + o.unattributedBytes,
    );
  }
  return {
    path: o.path,
    codeBytes: o.codeBytes,
    owners: [...owners.entries()]
      .map(([owner, bytes]) => ({ owner, bytes }))
      .sort((a, b) => b.bytes - a.bytes),
  };
}

/** Outputs reachable from `roots` by static imports, and dynamic ones too if `dynamic`. */
function closure(
  byPath: Map<string, AnalyzedOutput>,
  roots: string[],
  dynamic: boolean,
): Set<string> {
  const seen = new Set<string>();
  const queue = [...roots];
  while (queue.length > 0) {
    const next = queue.pop()!;
    if (seen.has(next)) continue;
    seen.add(next);
    const output = byPath.get(next);
    queue.push(...(output?.imports ?? []));
    if (dynamic) queue.push(...(output?.dynamicImports ?? []));
  }
  return seen;
}
