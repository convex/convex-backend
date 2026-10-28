import { Command } from "@commander-js/extra-typings";
import { components } from "./commands/components.js";
import { explore } from "./commands/explore.js";
import { files } from "./commands/files.js";
import { insights } from "./commands/insights.js";
import { modules } from "./commands/modules.js";
import { packages } from "./commands/packages.js";
import { summary } from "./commands/summary.js";
import { AnalyzerError } from "./lib/output.js";
import { analyzerVersion } from "./version.js";

const program = new Command("bundle-analyzer")
  .description(
    [
      "Analyze the code bundle for the Convex project in the current directory.",
      "",
      "This package is in alpha. Commands and output shapes may change between releases.",
    ].join("\n"),
  )
  .version(analyzerVersion)
  .addCommand(summary)
  .addCommand(components)
  .addCommand(packages)
  .addCommand(files)
  .addCommand(modules)
  .addCommand(insights)
  .addCommand(explore);

program.parseAsync(process.argv).catch((e: unknown) => {
  if (e instanceof AnalyzerError) {
    process.stderr.write(`✖ ${e.message}\n`);
    process.exit(1);
  }
  throw e;
});
