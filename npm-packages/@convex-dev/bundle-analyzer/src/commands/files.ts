import { Command } from "@commander-js/extra-typings";
import { logOutput, parsePositiveInteger, printJson } from "../lib/output.js";
import {
  AnalyzedBuild,
  InputAttribution,
  displayPath,
  inputBytes,
  ownerLabel,
} from "../lib/model.js";
import {
  bar,
  barCells,
  bold,
  colored,
  dim,
  formatBytes,
  heading,
  ownerColor,
  plural,
  prose,
  table,
  terminalWidth,
  truncate,
  wrapItems,
} from "../lib/format.js";
import { loadAnalysis } from "../lib/analysis.js";
import { DEFAULT_CHAINS, DEFAULT_LIMIT, jsonOption } from "../lib/command.js";
import {
  ChainTarget,
  chainTargets,
  chainTargetsJson,
  importedBySection,
} from "../lib/importChains.js";

export const files = new Command("files")
  .summary("Largest individual source files across your bundles")
  .description(
    [
      "Largest individual source files across your bundles.",
      "",
      "Files are sorted by size (code and source maps).",
      "",
      "Pass a file to see each module the file is bundled into and the entry points that import it, each with its shortest import chain.",
    ].join("\n"),
  )
  .argument(
    "[file]",
    "File path, or part of one (e.g. `convex/lib/` or `zod@3.25.76/index.js`)",
  )
  .option(
    "--limit <n>",
    `Number of rows in each list: files, matching files and their copies, and entry points per build (default: ${DEFAULT_LIMIT}, ${DEFAULT_CHAINS} entry points)`,
    parsePositiveInteger,
  )
  .addOption(jsonOption())
  .action(async (file, options) => {
    const { builds } = await loadAnalysis();
    const limit = options.limit ?? DEFAULT_LIMIT;
    const chains = options.limit ?? DEFAULT_CHAINS;
    if (file !== undefined) {
      const found = fileDetails(builds, file, limit);
      if (options.json) {
        printJson(fileDetailsJson(found, { limit, chains }));
      } else {
        logOutput(fileDetailsReport(found, file, { limit, chains }));
      }
    } else if (options.json) {
      printJson(filesJson(builds, limit));
    } else {
      logOutput(filesReport(builds, { limit }));
    }
  });

export function filesReport(
  builds: AnalyzedBuild[],
  options: { limit: number },
): string {
  const { limit } = options;
  const width = terminalWidth();
  const all = fileUsage(builds).filter((f) => f.bytes > 0);
  const max = Math.max(1, ...all.map((f) => f.bytes));
  const cells = barCells(width, 50, 16);
  const rows = all.slice(0, limit).map((f) => {
    const color = ownerColor(f.origin, f.packageName ?? ownerLabel(f));
    return [
      colored(color, f.display),
      formatBytes(f.bytes),
      dim(formatBytes(f.codeBytes)),
      dim(formatBytes(f.sourceMapBytes)),
      bar(f.bytes / max, cells, color),
    ];
  });
  const hidden = all.length - rows.length;
  return [
    wrapItems(
      [bold("Files"), dim(plural(all.length, "file"))],
      dim(" · "),
      width,
    ),
    "",
    table(
      [
        { header: "File", flexible: true },
        { header: "Size", align: "right" },
        { header: "Code", align: "right", optional: true },
        { header: "Source maps", align: "right", optional: true },
        { header: "", optional: true },
      ],
      rows,
      width,
    ),
    ...(hidden > 0 ? [dim(`… ${hidden} more (use --limit)`)] : []),
  ].join("\n");
}

export function filesJson(builds: AnalyzedBuild[], limit: number) {
  return fileUsage(builds)
    .filter((f) => f.bytes > 0)
    .slice(0, limit)
    .map((f) => ({
      path: f.path,
      display: f.display,
      origin: f.origin,
      package: f.packageName,
      version: f.packageVersion,
      bytes: f.bytes,
      codeBytes: f.codeBytes,
      sourceMapBytes: f.sourceMapBytes,
      copies: f.copies,
    }));
}

/** One copy of a file: its code in one module of one build. */
export type FileCopy = {
  buildId: string;
  module: string;
  /** Code plus source map bytes. */
  bytes: number;
  codeBytes: number;
  sourceMapBytes: number;
};

export type FileUsage = InputAttribution & {
  /** Metafile input path. */
  path: string;
  /** The path with its package, e.g. `zod@3.25.76/index.js`. */
  display: string;
  /** Code plus source map bytes, summed over copies. */
  bytes: number;
  codeBytes: number;
  sourceMapBytes: number;
  /** Largest first. */
  copies: FileCopy[];
};

/**
 * Each input file, summed over every module and build it's bundled into. A
 * file attributed differently in different builds (a component's own code is
 * `your code` in its builds but the component's package in the app's) gets a
 * row per attribution, the same split `packages` makes.
 */
export function fileUsage(builds: AnalyzedBuild[]): FileUsage[] {
  const byKey = new Map<string, FileUsage>();
  for (const b of builds) {
    for (const o of b.outputs) {
      for (const i of o.inputs) {
        const key = `${i.path}\0${ownerLabel(i)}`;
        let usage = byKey.get(key);
        if (!usage) {
          usage = {
            origin: i.origin,
            packageName: i.packageName,
            packageVersion: i.packageVersion,
            packageDir: i.packageDir,
            path: i.path,
            display: displayPath(i.path, i),
            bytes: 0,
            codeBytes: 0,
            sourceMapBytes: 0,
            copies: [],
          };
          byKey.set(key, usage);
        }
        const bytes = inputBytes(i);
        usage.bytes += bytes;
        usage.codeBytes += i.bytesInOutput;
        usage.sourceMapBytes += i.sourceMapBytes;
        usage.copies.push({
          buildId: b.id,
          module: o.path,
          bytes,
          codeBytes: i.bytesInOutput,
          sourceMapBytes: i.sourceMapBytes,
        });
      }
    }
  }
  const result = [...byKey.values()];
  for (const f of result) f.copies.sort((a, b) => b.bytes - a.bytes);
  return result.sort(
    (a, b) =>
      b.bytes - a.bytes ||
      a.display.localeCompare(b.display) ||
      a.path.localeCompare(b.path),
  );
}

export type FileDetail = FileUsage & {
  /** Import chains to the file, by build. */
  chains: ChainTarget[];
};

/**
 * Files whose display path (e.g. `zod@3.25.76/index.js`) or input path
 * matches `query`, with their import chains. An exact match wins; otherwise
 * `query` matches part of the path. Only the `limit` largest matches are
 * returned, with the number of matches.
 */
export function fileDetails(
  builds: AnalyzedBuild[],
  query: string,
  limit: number,
): { files: FileDetail[]; matches: number } {
  const all = fileUsage(builds).filter((f) => f.bytes > 0);
  const exact = all.filter((f) => f.display === query || f.path === query);
  const matches =
    exact.length > 0
      ? exact
      : all.filter((f) => f.display.includes(query) || f.path.includes(query));
  const shown = matches.slice(0, limit);
  const targets = chainTargets(builds, {
    kind: "files",
    inputPaths: new Set(shown.map((f) => f.path)),
  });
  return {
    files: shown.map((f) => {
      // A file attributed differently in another build has its own row.
      const buildIds = new Set(f.copies.map((c) => c.buildId));
      return {
        ...f,
        chains: targets.filter(
          (t) => t.inputPath === f.path && buildIds.has(t.buildId),
        ),
      };
    }),
    matches: matches.length,
  };
}

export function fileDetailsJson(
  found: { files: FileDetail[]; matches: number },
  options: { limit: number; chains: number },
) {
  return {
    matches: found.matches,
    files: found.files.map((f) => ({
      path: f.path,
      display: f.display,
      origin: f.origin,
      package: f.packageName,
      version: f.packageVersion,
      bytes: f.bytes,
      codeBytes: f.codeBytes,
      sourceMapBytes: f.sourceMapBytes,
      copies: f.copies.slice(0, options.limit),
      copyCount: f.copies.length,
      chains: chainTargetsJson(f.chains, options.chains),
    })),
  };
}

export function fileDetailsReport(
  found: { files: FileDetail[]; matches: number },
  query: string,
  options: { limit: number; chains: number },
): string {
  const { limit, chains } = options;
  const width = terminalWidth();
  if (found.files.length === 0) {
    return [
      prose(`No bundled file matches ${bold(query)}.`, width),
      dim(
        prose(
          "Use part of a file path (e.g. `convex/lib/`) or a path with its package (e.g. `zod@3.25.76/index.js`).",
          width,
        ),
      ),
    ].join("\n");
  }
  const out: string[] = [];
  for (const f of found.files) {
    out.push(heading(f.display).trimEnd());
    // Files from same-version copies of a package share a display path.
    if (found.files.some((o) => o !== f && o.display === f.display)) {
      out.push(truncate(dim(`installed at ${f.path}`), width, "middle"));
    }
    out.push(
      dim(
        wrapItems(
          [
            formatBytes(f.bytes),
            `${formatBytes(f.codeBytes)} code`,
            `${formatBytes(f.sourceMapBytes)} source maps`,
            f.copies.length === 1 ? "1 copy" : `${f.copies.length} copies`,
          ],
          " · ",
          width,
        ),
      ),
      "",
      table(
        [
          { header: "Build", flexible: true },
          { header: "Module", flexible: true },
          { header: "Size", align: "right" },
          { header: "Code", align: "right", optional: true },
          { header: "Source maps", align: "right", optional: true },
        ],
        f.copies
          .slice(0, limit)
          .map((c) => [
            dim(c.buildId),
            c.module,
            formatBytes(c.bytes),
            dim(formatBytes(c.codeBytes)),
            dim(formatBytes(c.sourceMapBytes)),
          ]),
        width,
      ),
      ...(f.copies.length > limit
        ? [dim(`… ${f.copies.length - limit} more (use --limit)`)]
        : []),
      "",
      ...importedBySection(f.chains, { chains, width, showTargets: false }),
    );
  }
  const hidden = found.matches - found.files.length;
  if (hidden > 0) {
    out.push(
      "",
      dim(`… ${plural(hidden, "more matching file")} (use --limit)`),
    );
  }
  out.push(
    "",
    dim(
      prose(
        "Run `npx @convex-dev/bundle-analyzer modules <module>` to see what a module loads, or which modules load a chunk.",
        width,
      ),
    ),
  );
  return out.join("\n").trimStart();
}
