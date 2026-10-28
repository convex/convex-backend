import { Command } from "@commander-js/extra-typings";
import { logOutput, parsePositiveInteger, printJson } from "../lib/output.js";
import {
  Analysis,
  AnalyzedBuild,
  SourceOrigin,
  inputBytes,
  distinctExternalPackages,
  unmatchedSourceMapBytes,
} from "../lib/model.js";
import {
  CODE_COLOR,
  EXTERNAL_COLOR,
  MAP_COLOR,
  bold,
  dim,
  formatBytes,
  heading,
  indent,
  layoutWidth,
  legend,
  plural,
  stackedBar,
  terminalWidth,
  truncate,
  wrapItems,
} from "../lib/format.js";
import { loadAnalysis } from "../lib/analysis.js";
import {
  groupBySharedChunks,
  moduleLoads,
  modulesTable,
} from "../lib/modules.js";
import { packageUsage, packagesJson, packagesTable } from "../lib/packages.js";
import {
  CODE_SIZE_LIMITS_DOCS,
  DEFAULT_LIMIT,
  EXTERNAL_PACKAGES_DOCS,
  jsonOption,
} from "../lib/command.js";
import {
  componentSizes,
  componentsJson,
  componentsTable,
} from "./components.js";
import {
  findInsights,
  insightTitles,
  readDevDependencies,
} from "../lib/insights.js";

export const summary = new Command("summary")
  .summary("Overview of your bundle")
  .option(
    "--limit <n>",
    "Number of insights, components, packages and modules to show",
    parsePositiveInteger,
    10,
  )
  .addOption(jsonOption())
  .action(async (options) => {
    const { analysis, builds } = await loadAnalysis();
    const devDependencies = readDevDependencies(analysis.bundle.projectRoot);
    if (options.json) {
      printJson(
        summaryJson(analysis, builds, {
          limit: options.limit,
          devDependencies,
        }),
      );
    } else {
      logOutput(
        summaryReport(analysis, builds, {
          limit: options.limit,
          devDependencies,
        }),
      );
    }
  });

export function summaryReport(
  analysis: Analysis,
  builds: AnalyzedBuild[],
  options: { limit: number; devDependencies?: ReadonlySet<string> },
): string {
  const { limit } = options;
  const width = terminalWidth();
  const barWidth = Math.max(10, layoutWidth() - 4);
  const t = totals(builds);
  const components = new Set(builds.map((b) => b.component.label));
  const out: string[] = [];

  out.push(
    wrapItems(
      [
        bold("Convex bundle"),
        plural(components.size, "component"),
        plural(t.modules, "module"),
        dim(`bundled as convex ${analysis.bundle.convexVersion}`),
      ],
      dim(" · "),
      width,
    ),
  );
  out.push("");

  const uploadedBytes = t.codeBytes + t.sourceMapBytes;
  const size = uploadedBytes + t.externalPackageBytes;
  out.push(truncate(`  ${bold("Total size")} ${formatBytes(size)}`, width));
  out.push(
    dim(
      wrapItems(
        `Code + source maps${t.externalPackageBytes > 0 ? " + external packages (installed estimate)" : ""}`.split(
          " ",
        ),
        " ",
        width,
        "  ",
      ),
    ),
  );
  const sizeSegments = [
    { label: "code", color: CODE_COLOR, value: t.codeBytes },
    { label: "source maps", color: MAP_COLOR, value: t.sourceMapBytes },
    {
      label: "external packages",
      color: EXTERNAL_COLOR,
      value: t.externalPackageBytes,
    },
  ].filter((s) => s.value > 0);
  out.push("  " + stackedBar(sizeSegments, barWidth));
  out.push(
    legend(
      sizeSegments.map((s) => ({ ...s, value: formatBytes(s.value) })),
      width,
      "  ",
    ),
  );
  out.push(
    dim(
      wrapItems(
        [
          `Code and source maps total ${formatBytes(uploadedBytes)}.`,
          ...(t.externalPackageBytes > 0
            ? [
                `External package sizes are estimated from your node_modules and may differ in size when installed on the server. For more information, see: ${EXTERNAL_PACKAGES_DOCS}`,
              ]
            : []),
          `For code size limits, see: ${CODE_SIZE_LIMITS_DOCS}`,
          ...(t.unmeasuredExternalPackages > 0
            ? [
                `${plural(t.unmeasuredExternalPackages, "external package")} couldn't be found in your local node_modules, so ${t.unmeasuredExternalPackages === 1 ? "its size isn't" : "their sizes aren't"} included.`,
              ]
            : []),
        ]
          .join(" ")
          .split(" "),
        " ",
        width,
        "  ",
      ),
    ),
  );

  // Each list shows `limit` of its `total` rows, then how many more there are
  // and the command that lists them all, with `--limit` when the command's
  // `commandRows` are more than it shows by default. Commands without
  // `--limit` pass null.
  const seeAll = (
    total: number,
    command: string,
    what: string,
    commandRows: number | null = total,
  ) =>
    dim(
      wrapItems(
        `${total > limit ? `… ${total - limit} more. ` : ""}Run \`npx @convex-dev/bundle-analyzer ${command}${commandRows !== null && commandRows > DEFAULT_LIMIT ? ` --limit ${commandRows}` : ""}\` to see ${what}.`.split(
          " ",
        ),
        " ",
        width,
        "  ",
      ),
    );

  const found = findInsights(
    builds,
    options.devDependencies,
    analysis.bundle.projectRoot,
  );
  out.push(heading("Insights"));
  if (found.length === 0) {
    out.push(dim("  No insights."));
  } else {
    out.push(insightTitles(found.slice(0, limit), width));
    out.push(
      seeAll(found.length, "insights", "every insight with details", null),
    );
  }

  out.push(heading(`Components ${dim("(code and source maps)")}`));
  out.push(indent(componentsTable(builds, limit, width - 2)));
  out.push(
    seeAll(componentSizes(builds).length, "components", "every component"),
  );

  out.push(heading(`Largest packages ${dim("(code and source maps)")}`));
  out.push(indent(packagesTable(builds, limit, width - 2)));
  out.push(
    seeAll(
      packageUsage(builds).filter((p) => p.bytes > 0).length,
      "packages",
      "every package",
    ),
  );

  out.push(
    heading(
      `Heaviest modules ${dim("(code in each entry point plus the shared chunks it imports)")}`,
    ),
  );
  out.push(indent(modulesTable(builds, limit, width - 2, { grouped: true })));
  const loads = moduleLoads(builds);
  out.push(
    seeAll(
      groupBySharedChunks(loads).length,
      "modules",
      "every module",
      loads.length,
    ),
  );

  return out.join("\n");
}

export function summaryJson(
  analysis: Analysis,
  builds: AnalyzedBuild[],
  options: { limit: number; devDependencies?: ReadonlySet<string> },
) {
  const { limit } = options;
  const t = totals(builds);
  return {
    convexVersion: analysis.bundle.convexVersion,
    includeSourcesContent: analysis.bundle.includeSourcesContent,
    totals: {
      ...t,
      bytes: t.codeBytes + t.sourceMapBytes,
    },
    components: componentsJson(builds, limit),
    topPackages: packagesJson(builds, limit),
    heaviestModules: groupBySharedChunks(moduleLoads(builds))
      .slice(0, limit)
      .map(([heaviest, ...rest]) => ({
        ...heaviest,
        sameChunks: rest.map((m) => m.path),
      })),
    insights: findInsights(
      builds,
      options.devDependencies,
      analysis.bundle.projectRoot,
    ).slice(0, limit),
  };
}

export type Totals = {
  modules: number;
  codeBytes: number;
  sourceMapBytes: number;
  /**
   * Unzipped size on disk of distinct external node packages. They're
   * installed on the server rather than uploaded.
   */
  externalPackageBytes: number;
  /**
   * Distinct external packages missing from the local node_modules, whose
   * size `externalPackageBytes` leaves out.
   */
  unmeasuredExternalPackages: number;
  /** Code and source map bytes by input origin. */
  byOrigin: Record<SourceOrigin, number>;
  /** Code bytes not attributed to any input. */
  codeGlueBytes: number;
  /** Source map bytes that belong to no source. */
  sourceMapOverheadBytes: number;
  /** Source map bytes for sources that don't match any input. */
  sourceMapUnmatchedBytes: number;
};

function emptyOriginMap(): Record<SourceOrigin, number> {
  return { app: 0, generated: 0, convex: 0, npm: 0, virtual: 0 };
}

export function totals(builds: AnalyzedBuild[]): Totals {
  const t: Totals = {
    modules: 0,
    codeBytes: 0,
    sourceMapBytes: 0,
    externalPackageBytes: 0,
    unmeasuredExternalPackages: 0,
    byOrigin: emptyOriginMap(),
    codeGlueBytes: 0,
    sourceMapOverheadBytes: 0,
    sourceMapUnmatchedBytes: 0,
  };
  for (const b of builds) {
    for (const o of b.outputs) {
      t.modules += 1;
      t.codeBytes += o.codeBytes;
      t.sourceMapBytes += o.sourceMapBytes;
      for (const i of o.inputs) t.byOrigin[i.origin] += inputBytes(i);
      t.codeGlueBytes += o.unattributedBytes;
      t.sourceMapOverheadBytes += o.sourceMapOverheadBytes;
      t.sourceMapUnmatchedBytes += unmatchedSourceMapBytes(o);
    }
  }
  for (const e of distinctExternalPackages(builds)) {
    if (e.diskBytes === null) t.unmeasuredExternalPackages += 1;
    else t.externalPackageBytes += e.diskBytes;
  }
  return t;
}
