import { Command } from "@commander-js/extra-typings";
import { runExplorer } from "../lib/tui.js";
import { AnalyzerError } from "../lib/output.js";
import { loadAnalysis } from "../lib/analysis.js";

export const explore = new Command("explore")
  .summary("Browse your bundles interactively in the terminal")
  .description(
    [
      "Browse your bundles in an interactive terminal UI: drill down from components and runtimes to modules, packages and files, with a live treemap of the current level and the import chain that pulls in each file.",
      "",
      "Total size includes code, source maps, and installed external package estimates. Press t to switch layouts, g to group by component or by package, / to filter, and ? for all keys.",
    ].join("\n"),
  )
  .action(async () => {
    if (!process.stdin.isTTY || !process.stdout.isTTY) {
      throw new AnalyzerError(
        "`explore` requires an interactive terminal. To print a report, use the other commands with --json for JSON output.",
      );
    }
    const { builds } = await loadAnalysis();
    await runExplorer(builds);
  });
