import { Sizes } from "./sizes.js";
import {
  AnalyzedBuild,
  SourceOrigin,
  inputBytes,
  ownerLabel,
} from "./model.js";
import {
  bar,
  barCells,
  colored,
  dim,
  formatBytes,
  formatPercent,
  ownerColor,
  table,
  terminalWidth,
} from "./format.js";

export function packagesTable(
  builds: AnalyzedBuild[],
  top: number,
  width = terminalWidth(),
): string {
  const all = packageUsage(builds).filter((p) => p.bytes > 0);
  const grand = Math.max(
    1,
    all.reduce((n, p) => n + p.bytes, 0),
  );
  const max = Math.max(1, ...all.map((p) => p.bytes));
  const cells = barCells(width, 70, 16);
  const kinds = new Map(builds.map((b) => [b.id, b.kind]));
  const inRuntime = (p: PackageUsage, kind: AnalyzedBuild["kind"]) => {
    let n = 0;
    for (const [id, sizes] of p.builds) {
      if (kinds.get(id) === kind) n += sizes.bytes;
    }
    return n > 0 ? formatBytes(n) : dim("-");
  };
  const rows = all.slice(0, top).map((p, idx) => {
    const color = ownerColor(p.origin, p.name);
    return [
      dim(String(idx + 1)),
      colored(color, p.key),
      formatBytes(p.bytes),
      inRuntime(p, "convex"),
      inRuntime(p, "node"),
      formatPercent(p.bytes / grand),
      String(p.files),
      bar(p.bytes / max, cells, color),
    ];
  });
  return table(
    [
      { header: "#", align: "right" },
      { header: "Package", flexible: true, truncate: "end" },
      { header: "Size", align: "right" },
      { header: "Convex (incl. maps)", align: "right", optional: true },
      { header: "Node (incl. maps)", align: "right", optional: true },
      { header: "Share", align: "right" },
      { header: "Files", align: "right" },
      { header: "", optional: true },
    ],
    rows,
    width,
  );
}

export function packagesJson(builds: AnalyzedBuild[], limit: number) {
  return packageUsage(builds)
    .filter((p) => p.bytes > 0)
    .slice(0, limit)
    .map((p) => ({
      package: p.key,
      name: p.name,
      version: p.version,
      origin: p.origin,
      bytes: p.bytes,
      codeBytes: p.codeBytes,
      sourceMapBytes: p.sourceMapBytes,
      files: p.files,
      byBuild: Object.fromEntries(p.builds),
      packageDirs: [...p.packageDirs],
    }));
}

export type PackageUsage = {
  key: string;
  name: string;
  version: string | null;
  origin: SourceOrigin;
  /** Code plus source map bytes. */
  bytes: number;
  codeBytes: number;
  sourceMapBytes: number;
  /** Distinct files, counting a file bundled into several builds once. */
  files: number;
  /** Sizes in each build the package appears in, by build id. */
  builds: Map<string, Sizes>;
  packageDirs: Set<string>;
};

export function packageUsage(builds: AnalyzedBuild[]): PackageUsage[] {
  const byKey = new Map<string, PackageUsage>();
  const filesByKey = new Map<string, Set<string>>();
  for (const b of builds) {
    for (const o of b.outputs) {
      for (const i of o.inputs) {
        const key = ownerLabel(i);
        let usage = byKey.get(key);
        if (!usage) {
          usage = {
            key,
            name: i.packageName ?? key,
            version: i.packageVersion,
            origin: i.origin,
            bytes: 0,
            codeBytes: 0,
            sourceMapBytes: 0,
            files: 0,
            builds: new Map(),
            packageDirs: new Set(),
          };
          byKey.set(key, usage);
        }
        const bytes = inputBytes(i);
        usage.bytes += bytes;
        usage.codeBytes += i.bytesInOutput;
        usage.sourceMapBytes += i.sourceMapBytes;
        const seen = filesByKey.get(key) ?? new Set();
        seen.add(i.path);
        filesByKey.set(key, seen);
        usage.files = seen.size;
        const inBuild = usage.builds.get(b.id) ?? {
          bytes: 0,
          codeBytes: 0,
          sourceMapBytes: 0,
        };
        inBuild.bytes += bytes;
        inBuild.codeBytes += i.bytesInOutput;
        inBuild.sourceMapBytes += i.sourceMapBytes;
        usage.builds.set(b.id, inBuild);
        if (i.packageDir) usage.packageDirs.add(i.packageDir);
      }
    }
  }
  return [...byKey.values()].sort((a, b) => b.bytes - a.bytes);
}
