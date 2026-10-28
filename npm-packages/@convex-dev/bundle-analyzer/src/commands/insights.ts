import { Command } from "@commander-js/extra-typings";
import { logOutput, printJson } from "../lib/output.js";
import { loadAnalysis } from "../lib/analysis.js";
import { jsonOption } from "../lib/command.js";
import {
  findInsights,
  insightsReport,
  readDevDependencies,
} from "../lib/insights.js";

export const insights = new Command("insights")
  .summary("Suggestions for making your bundles smaller")
  .description(
    [
      "Suggestions for making your bundles smaller.",
      "",
      "Insights are sorted by how much code following them would save. Sizes include code and source maps, shown separately.",
    ].join("\n"),
  )
  .addOption(jsonOption())
  .action(async (options) => {
    const { analysis, builds } = await loadAnalysis();
    const found = findInsights(
      builds,
      readDevDependencies(analysis.bundle.projectRoot),
      analysis.bundle.projectRoot,
    );
    if (options.json) {
      printJson({ insights: found });
    } else {
      logOutput(found.length > 0 ? insightsReport(found) : "No insights.");
    }
  });
