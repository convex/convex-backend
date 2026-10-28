import { Command, Option } from "@commander-js/extra-typings";
import { oneoffContext } from "../bundler/context.js";
import { runCodegen } from "./lib/components.js";
import { getDeploymentSelection } from "./lib/deploymentSelection.js";
import {
  DetailedDeploymentCredentials,
  loadSelectedDeploymentCredentials,
} from "./lib/api.js";
import { withRunningBackend } from "./lib/localDeployment/run.js";
export const codegen = new Command("codegen")
  .summary("Generate backend type definitions")
  .description(
    [
      "Generate code in `convex/_generated/` based on the current contents of `convex/`.",
      "",
      "This code is generated automatically while running `npx convex dev` and should be committed to the repo (your code won't typecheck without it!). Regenerating it explicitly is rarely needed (e.g. in CI to ensure the correct code was checked in).",
      "",
      "This doesn't modify the code running on the deployment.",
    ].join("\n"),
  )
  .allowExcessArguments(false)
  .option(
    "--dry-run",
    "Print out the generated configuration to stdout instead of writing to convex directory",
  )
  .addOption(new Option("--debug").hideHelp())
  .addOption(
    new Option(
      "--typecheck <mode>",
      `Whether to check TypeScript files with \`tsc --noEmit\`.`,
    )
      .choices(["enable", "try", "disable"] as const)
      .default("try" as const),
  )
  .option(
    "--init",
    "Also (over-)write the default convex/README.md and convex/tsconfig.json files, otherwise only written when creating a new Convex project.",
  )
  .addOption(new Option("--admin-key <adminKey>").hideHelp())
  .addOption(new Option("--url <url>").hideHelp())
  .addOption(new Option("--live-component-sources").hideHelp())
  // Experimental option
  .addOption(
    new Option(
      "--commonjs",
      "Generate CommonJS modules (CJS) instead of ECMAScript modules, the default. Bundlers typically take care of this conversion while bundling, so this setting is generally only useful for projects which do not use a bundler, typically Node.js projects. Convex functions can be written with either syntax.",
    ).hideHelp(),
  )
  // Only for doing codegen on system UDFs
  .addOption(new Option("--system-udfs").hideHelp())
  .option(
    "--component-dir <path>",
    "Generate code for a specific component directory instead of the current application.",
  )
  .action(async (options) => {
    const ctx = await oneoffContext(options);
    const deploymentSelection = await getDeploymentSelection(ctx, options);

    const codegenOptions = {
      dryRun: !!options.dryRun,
      debug: !!options.debug,
      typecheck: options.typecheck,
      init: !!options.init,
      commonjs: !!options.commonjs,
      url: options.url,
      adminKey: options.adminKey,
      liveComponentSources: !!options.liveComponentSources,
      debugNodeApis: false,
      systemUdfs: !!options.systemUdfs,
      codegenOnlyThisComponent: options.componentDir,
    };

    if (options.systemUdfs) {
      await runCodegen(ctx, null, codegenOptions);
      return;
    }

    // Early exit for a better error message trying to use a preview key.
    if (deploymentSelection.kind === "preview") {
      return await ctx.crash({
        exitCode: 1,
        errorType: "invalid filesystem data",
        printedMessage: `Codegen requires an existing deployment so doesn't support CONVEX_DEPLOY_KEY.\nGenerate code in dev and commit it to the repo instead.\nhttps://docs.convex.dev/understanding/best-practices/other-recommendations#check-generated-code-into-version-control`,
      });
    }

    const credentials: DetailedDeploymentCredentials =
      await loadSelectedDeploymentCredentials(ctx, deploymentSelection, {
        ensureLocalRunning: false,
      });

    await withRunningBackend({
      ctx,
      deployment: {
        deploymentUrl: credentials.url,
        deploymentFields: credentials.deploymentFields,
      },
      action: async () => {
        await runCodegen(ctx, credentials, codegenOptions);
      },
    });
  });
