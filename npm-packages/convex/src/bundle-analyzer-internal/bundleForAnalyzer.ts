import path from "path";
import { Context } from "../bundler/context.js";
import { changeSpinner } from "../bundler/log.js";
import {
  ProjectConfig,
  getFunctionsDirectoryPath,
  readProjectConfig,
} from "../cli/lib/config.js";
import { ensureHasConvexDependency } from "../cli/lib/utils/utils.js";
import {
  BuildDebugInfo,
  bundleImplementations,
  componentGraph,
} from "../cli/lib/components/definition/bundle.js";
import { isComponentDirectory } from "../cli/lib/components/definition/directoryStructure.js";

export type ProjectBundle = {
  projectConfig: ProjectConfig;
  /** The directory esbuild resolves metafile input paths against. */
  absWorkingDir: string;
  builds: BuildDebugInfo[];
};

/**
 * Bundles every component in the project in the current directory. Does not run codegen, so `convex/_generated` must already exist.
 */
export async function bundleForAnalyzer(
  ctx: Context,
  options: { cmd: string },
): Promise<ProjectBundle> {
  await ensureHasConvexDependency(ctx, options.cmd);
  const { projectConfig } = await readProjectConfig(ctx);
  const convexDir = await getFunctionsDirectoryPath(ctx);
  if (!ctx.fs.exists(path.join(convexDir, "_generated"))) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `${path.relative(".", convexDir) || "."}/_generated is missing. Run \`npx convex dev --once\` or \`npx convex codegen\` first.`,
    });
  }
  const absWorkingDir = path.resolve(".");
  const isComponent = isComponentDirectory(ctx, convexDir, true);
  if (isComponent.kind === "err") {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Invalid component root directory (${isComponent.why}): ${convexDir}`,
    });
  }
  const rootComponent = isComponent.component;

  changeSpinner("Finding component definitions...");
  const { components } = await componentGraph(
    ctx,
    absWorkingDir,
    rootComponent,
    false,
    false,
  );

  changeSpinner("Bundling functions for every component...");
  const { buildDebugInfos } = await bundleImplementations({
    ctx,
    rootComponentDirectory: rootComponent,
    componentDirectories: [...components.values()].filter((d) => !d.isRoot),
    nodeExternalPackages: projectConfig.node.externalPackages,
    extraConditions: [],
    verbose: false,
    includeSourcesContent:
      projectConfig.bundler?.includeSourcesContent ?? false,
  });
  return { projectConfig, absWorkingDir, builds: buildDebugInfos };
}
