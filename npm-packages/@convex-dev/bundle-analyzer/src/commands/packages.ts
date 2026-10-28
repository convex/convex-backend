import { Command } from "@commander-js/extra-typings";
import { logOutput, parsePositiveInteger, printJson } from "../lib/output.js";
import { AnalyzedBuild, ownerLabel } from "../lib/model.js";
import {
  bold,
  dim,
  formatBytes,
  plural,
  prose,
  terminalWidth,
  wrapItems,
} from "../lib/format.js";
import { loadAnalysis } from "../lib/analysis.js";
import {
  DEFAULT_CHAINS,
  DEFAULT_LIMIT,
  EXTERNAL_PACKAGES_DOCS,
  jsonOption,
} from "../lib/command.js";
import {
  ChainTarget,
  chainTargets,
  chainTargetsJson,
  importedBySection,
  matchesPackage,
} from "../lib/importChains.js";
import { Sizes, sizes, sumSizes } from "../lib/sizes.js";
import { inputGraph, reachable, sizesByInput } from "../lib/graph.js";
import { packagesTable, packagesJson, packageUsage } from "../lib/packages.js";

export const packages = new Command("packages")
  .summary("Largest npm packages (and your own code) across your bundles")
  .description(
    [
      "Largest npm packages (and your own code) across your bundles.",
      "",
      "Each version of a package is a separate row. Packages are sorted by code plus source maps.",
      "",
      "Pass a package to see the entry points that import each copy of it, each with its shortest import chain. Copies installed in different directories are listed separately.",
    ].join("\n"),
  )
  .argument(
    "[package]",
    "Package name, optionally with a version (e.g. `zod` or `zod@3.25.76`)",
  )
  .option(
    "--limit <n>",
    `Number of packages, or of entry points per copy of a package (default: ${DEFAULT_LIMIT} packages, ${DEFAULT_CHAINS} entry points)`,
    parsePositiveInteger,
  )
  .addOption(jsonOption())
  .action(async (name, options) => {
    const { builds } = await loadAnalysis();
    const limit = options.limit ?? DEFAULT_LIMIT;
    const chains = options.limit ?? DEFAULT_CHAINS;
    if (name !== undefined) {
      const detail = packageDetail(builds, name);
      if (options.json) {
        printJson(packageDetailJson(detail, { chains }));
      } else {
        logOutput(packageDetailReport(detail, { chains }));
      }
    } else if (options.json) {
      printJson(packagesJson(builds, limit));
    } else {
      logOutput(packagesReport(builds, { limit }));
    }
  });

export function packagesReport(
  builds: AnalyzedBuild[],
  options: { limit: number },
): string {
  const { limit } = options;
  const all = packageUsage(builds).filter((p) => p.bytes > 0);
  const total = all.reduce((n, p) => n + p.bytes, 0);
  const width = terminalWidth();
  return [
    wrapItems(
      [
        bold("Packages"),
        dim(plural(all.length, "package")),
        dim(`${formatBytes(total)} of code and source maps`),
      ],
      dim(" · "),
      width,
    ),
    "",
    packagesTable(builds, limit, width),
    ...(all.length > limit
      ? [dim(`… ${all.length - limit} more (use --limit)`)]
      : []),
    "",
    dim(
      prose(
        "Run `npx @convex-dev/bundle-analyzer packages <package>` to see what imports a package.",
        width,
      ),
    ),
  ].join("\n");
}

export type PackageDetail = {
  name: string;
  /** Each copy of the package in each build, with its import chains. */
  copies: ChainTarget[];
  /**
   * Directly imported packages, with code and source map bytes reachable
   * through those imports, largest first.
   */
  imports: (Sizes & { package: string })[];
  /** External node packages with this name, which aren't bundled. */
  external: { buildId: string; name: string; version: string }[];
};

export function packageDetail(
  builds: AnalyzedBuild[],
  name: string,
): PackageDetail {
  return {
    name,
    copies: chainTargets(builds, { kind: "package", name }),
    imports: packageImports(builds, name),
    external: builds.flatMap((b) =>
      b.externalPackages
        .filter((e) => e.name === name || `${e.name}@${e.version}` === name)
        .map((e) => ({ buildId: b.id, name: e.name, version: e.version })),
    ),
  };
}

export function packageDetailJson(
  detail: PackageDetail,
  options: { chains: number },
) {
  return {
    name: detail.name,
    copies: chainTargetsJson(detail.copies, options.chains),
    imports: detail.imports,
    external: detail.external.map((e) => ({
      build: e.buildId,
      name: e.name,
      version: e.version,
    })),
  };
}

export function packageDetailReport(
  detail: PackageDetail,
  options: { chains: number },
): string {
  const { chains } = options;
  const width = terminalWidth();
  const name = bold(detail.name);
  const externalIn = [...new Set(detail.external.map((e) => e.buildId))];
  if (detail.copies.length === 0) {
    if (externalIn.length > 0) {
      return prose(
        `${name} isn't bundled because it is an external package in ${externalIn.join(", ")}: ${EXTERNAL_PACKAGES_DOCS}`,
        width,
      );
    }
    return [
      prose(`No bundled package is named ${name}.`, width),
      dim(
        prose(
          "Use a package name (e.g. `zod`) or name and version (e.g. `zod@3.25.76`). To search file paths, use `npx @convex-dev/bundle-analyzer files <path>`.",
          width,
        ),
      ),
    ].join("\n");
  }

  const copies = new Set(
    detail.copies.map((c) => `${c.label}\0${c.packageDir}`),
  ).size;
  const builds = new Set(detail.copies.map((c) => c.buildId)).size;
  const total = detail.copies.reduce((n, c) => n + c.bytes, 0);
  const codeBytes = detail.copies.reduce((n, c) => n + c.codeBytes, 0);
  const out = [
    wrapItems(
      [
        name,
        dim(copies === 1 ? "1 copy" : `${copies} copies`),
        dim(plural(builds, "build")),
        dim(`${formatBytes(total)} of code and source maps`),
      ],
      dim(" · "),
      width,
    ),
  ];
  if (codeBytes === 0) {
    const bundled = detail.imports.filter((i) => i.bytes > 0);
    out.push(
      "",
      prose(
        `It adds no code of its own to this bundle.${
          bundled.length > 0
            ? ` Its imports reach: ${bundled
                .map(
                  (i) =>
                    `${i.package} (${formatBytes(i.codeBytes)} of code and ${formatBytes(i.sourceMapBytes)} of source maps)`,
                )
                .join(", ")}.`
            : ""
        }`,
        width,
      ),
    );
  }
  out.push(
    "",
    ...importedBySection(detail.copies, { chains, width, showTargets: true }),
  );
  if (externalIn.length > 0) {
    out.push(
      "",
      dim(
        prose(
          `It is also an external package in ${externalIn.join(", ")}: ${EXTERNAL_PACKAGES_DOCS}`,
          width,
        ),
      ),
    );
  }
  const labels = [...new Set(detail.copies.map((c) => c.label))];
  const commands = labels.map(
    (label) => `\`npx @convex-dev/bundle-analyzer files ${label}/\``,
  );
  out.push(
    "",
    dim(
      prose(
        `Use ${commands.join(" or ")} to see ${labels.length === 1 ? "its" : "each version's"} largest files.`,
        width,
      ),
    ),
  );
  return out.join("\n");
}

/** Sizes reachable through direct imports, scoped to each build and package. */
function packageImports(
  builds: AnalyzedBuild[],
  name: string,
): PackageDetail["imports"] {
  const result = new Map<string, Sizes>();
  for (const b of builds) {
    const imported = new Map<string, Set<string>>();
    for (const [file, input] of Object.entries(b.metafile.inputs)) {
      if (!matchesPackage(b.attribution.get(file), name)) continue;
      for (const imp of input.imports) {
        if (imp.external) continue;
        const a = b.attribution.get(imp.path);
        if (!a?.packageName || matchesPackage(a, name)) continue;
        const label = ownerLabel(a);
        const paths = imported.get(label) ?? new Set<string>();
        paths.add(imp.path);
        imported.set(label, paths);
      }
    }
    if (imported.size === 0) continue;
    const graph = inputGraph(b);
    const bytes = sizesByInput(b.outputs);
    for (const [label, paths] of imported) {
      const contributions = [...reachable(graph, paths)]
        .filter((file) => {
          const a = b.attribution.get(file);
          return a && ownerLabel(a) === label;
        })
        .map((file) => bytes.get(file) ?? sizes());
      result.set(
        label,
        sumSizes([result.get(label) ?? sizes(), ...contributions]),
      );
    }
  }
  return [...result]
    .map(([label, value]) => ({ package: label, ...value }))
    .sort((a, b) => b.bytes - a.bytes || a.package.localeCompare(b.package));
}
