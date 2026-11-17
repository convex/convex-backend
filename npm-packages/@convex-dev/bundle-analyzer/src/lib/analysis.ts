import { collectBundle } from "./collect.js";
import { bundle, analyzerConvexVersion } from "./bundle.js";
import { Analysis, AnalyzedBuild, analyze } from "./model.js";
import { dim } from "./format.js";
import { AnalyzerError, logWarning } from "./output.js";
import {
  findProjectConvexVersion,
  versionMismatchWarning,
} from "./versionCheck.js";

export async function loadAnalysis(): Promise<{
  analysis: Analysis;
  builds: AnalyzedBuild[];
}> {
  const projectConvexVersion = findProjectConvexVersion(process.cwd());
  if (projectConvexVersion === null) {
    throw new AnalyzerError(
      "Couldn't find the `convex` package. Run this from a Convex project directory with its dependencies installed.",
    );
  }
  const warning = versionMismatchWarning(
    projectConvexVersion,
    analyzerConvexVersion,
  );
  if (warning !== null) {
    logWarning(dim(warning));
  }
  const collected = collectBundle(await bundle());
  const analysis = analyze(collected);
  return { analysis, builds: analysis.builds };
}
