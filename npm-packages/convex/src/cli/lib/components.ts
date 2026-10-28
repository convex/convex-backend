import path from "path";
import { Context } from "../../bundler/context.js";
import {
  changeSpinner,
  logFinishedStep,
  logMessage,
} from "../../bundler/log.js";
import {
  ProjectConfig,
  configFromProjectConfig,
  debugIsolateEndpointBundles,
  getFunctionsDirectoryPath,
  readProjectConfig,
  pullConfig,
  diffConfig,
} from "./config.js";
import {
  finishPush,
  reportPushCompleted,
  startPush,
  waitForSchema,
} from "./deploy2.js";
import { version } from "../version.js";
import { ensureHasConvexDependency, functionsDir } from "./utils/utils.js";
import {
  bundleDefinitions,
  bundleImplementations,
  componentGraph,
} from "./components/definition/bundle.js";
import { isComponentDirectory } from "./components/definition/directoryStructure.js";
import {
  doFinalComponentCodegen,
  doInitialComponentCodegen,
  CodegenOptions,
  doInitConvexFolder,
  doCodegen,
} from "./codegen.js";
import {
  AppDefinitionConfig,
  ComponentDefinitionConfig,
} from "./deployApi/definitionConfig.js";
import { typeCheckFunctionsInMode, TypeCheckMode } from "./typecheck.js";
import { withTmpDir } from "../../bundler/fs.js";
import { handleDebugBundlePath } from "./debugBundlePath.js";
import { chalkStderr } from "chalk";
import {
  startPushResponseForLogging,
  redactEnvironmentVariableValues,
  StartPushRequest,
  StartPushResponse,
} from "./deployApi/startPush.js";
import { DetailedDeploymentCredentials } from "./api.js";
import { FinishPushDiff } from "./deployApi/finishPush.js";
import { Reporter, Span } from "./tracing.js";
import { DEFINITION_FILENAME_TS } from "./components/constants.js";
import { DeploymentType } from "./api.js";
import { deploymentDashboardUrlPage } from "./dashboard.js";
import { formatIndex, LargeIndexDeletionCheck } from "./indexes.js";
import { checkForLargeIndexDeletion } from "./checkForLargeIndexDeletion.js";
import { checkForSlowSchemaValidation } from "./checkForSlowSchemaValidation.js";
import {
  checkForLargeIndexBackfill,
  LargeIndexBackfillCheck,
} from "./checkForLargeIndexBackfill.js";
import {
  pushStagesValidators,
  sharedSchemaEvaluation,
} from "./schemaEvaluation.js";
import {
  checkForStagedValidatorDeletion,
  StagedValidatorDeletionCheck,
} from "./checkForStagedValidatorDeletion.js";
import { LogManager } from "./logs.js";
import { createHash } from "crypto";
import { Bundle, BundleHash } from "../../bundler/index.js";
import { ModuleHashConfig } from "./deployApi/modules.js";

export type PushOptions = {
  adminKey: string;
  verbose: boolean;
  dryRun: boolean;
  typecheck: "enable" | "try" | "disable";
  typecheckComponents: boolean;
  debug: boolean;
  debugBundlePath?: string | undefined;
  debugNodeApis: boolean;
  codegen: boolean;
  url: string;
  deploymentName: string | null;
  deploymentType?: DeploymentType;
  writePushRequest?: string | undefined;
  liveComponentSources: boolean;
  pushAllModules: boolean;
  logManager?: LogManager | undefined;
  largeIndexDeletionCheck: LargeIndexDeletionCheck;
  largeIndexBackfillCheck: LargeIndexBackfillCheck;
  stagedValidatorDeletionCheck: StagedValidatorDeletionCheck;
  // Whether a dry run should warn when the schema change would block the
  // deploy on a slow table walk. On for deploys; off for commands whose dry
  // runs never deploy (e.g. codegen).
  warnOnSlowSchemaValidation: boolean;
  message: string | null;
};

export async function runCodegen(
  ctx: Context,
  credentials: DetailedDeploymentCredentials | null,
  options: CodegenOptions,
) {
  // This also ensures the current directory is the project root.
  await ensureHasConvexDependency(ctx, "codegen");

  const { configPath, projectConfig } = await readProjectConfig(ctx);
  const functionsDirectoryPath = functionsDir(configPath, projectConfig);

  if (options.init) {
    await doInitConvexFolder(ctx, functionsDirectoryPath, {
      dryRun: options.dryRun,
      debug: options.debug,
    });
  }

  if (!options.systemUdfs) {
    if (credentials === null) {
      return;
    }

    await startComponentsPushAndCodegen(
      ctx,
      Span.noop(),
      projectConfig,
      configPath,
      {
        ...options,
        deploymentName: credentials.deploymentFields?.deploymentName ?? null,
        ...(credentials.deploymentFields?.deploymentType !== undefined
          ? { deploymentType: credentials.deploymentFields.deploymentType }
          : {}),
        url: credentials.url,
        adminKey: credentials.adminKey,
        generateCommonJSApi: options.commonjs,
        verbose: !!process.env.CONVEX_VERBOSE,
        codegen: true,
        liveComponentSources: options.liveComponentSources,
        typecheckComponents: false,
        debugNodeApis: options.debugNodeApis,
        // Code generation uses the source schema independently of deployment data.
        largeIndexDeletionCheck: "no verification",
        largeIndexBackfillCheck: "no verification",
        stagedValidatorDeletionCheck: "no verification",
        warnOnSlowSchemaValidation: false,
      },
    );
  } else {
    if (options.typecheck !== "disable") {
      logMessage(chalkStderr.gray("Running TypeScript typecheck…"));
    }

    await doCodegen(ctx, functionsDirectoryPath, options.typecheck, {
      dryRun: options.dryRun,
      debug: options.debug,
      generateCommonJSApi: options.commonjs,
    });
  }
}

export async function runPush(ctx: Context, options: PushOptions) {
  const { configPath, projectConfig } = await readProjectConfig(ctx);
  await runComponentsPush(ctx, options, configPath, projectConfig);
}

export function hash(bundle: Bundle) {
  return createHash("sha256")
    .update(bundle.source)
    .update(bundle.sourceMap || "")
    .digest("hex");
}

function isModuleTheSame(newBundle: Bundle, oldBundleHash: BundleHash) {
  return (
    newBundle.environment === oldBundleHash.environment &&
    hash(newBundle) === oldBundleHash.hash
  );
}

export function partitionModulesByChanges(
  functions: Bundle[],
  remoteHashesByPath: Map<string, BundleHash>,
): {
  unchangedModuleHashes: ModuleHashConfig[];
  changedModules: Bundle[];
} {
  // Partition modules based on whether they match the existing modules
  const unchangedModuleHashes = functions
    .filter((newBundle) => {
      const oldBundleHash = remoteHashesByPath.get(newBundle.path);
      return oldBundleHash && isModuleTheSame(newBundle, oldBundleHash);
    })
    .map((func) => ({
      path: func.path,
      environment: func.environment,
      sha256: hash(func),
    }));
  const changedModules = functions.filter((newBundle) => {
    const oldBundleHash = remoteHashesByPath.get(newBundle.path);
    return !oldBundleHash || !isModuleTheSame(newBundle, oldBundleHash);
  });
  return { unchangedModuleHashes, changedModules };
}

async function getUnchangedModuleHashesFromServer(
  ctx: Context,
  appImplementation: { functions: Bundle[] },
  options: { url: string; adminKey: string },
): Promise<{
  unchangedModuleHashes: ModuleHashConfig[];
  changedModules: Bundle[];
}> {
  const remoteConfigWithModuleHashes = await pullConfig(
    ctx,
    undefined,
    undefined,
    options.url,
    options.adminKey,
  );
  const remoteHashesByPath = new Map(
    remoteConfigWithModuleHashes.moduleHashes.map((moduleHash) => [
      moduleHash.path,
      moduleHash,
    ]),
  );
  return partitionModulesByChanges(
    appImplementation.functions,
    remoteHashesByPath,
  );
}

async function startComponentsPushAndCodegen(
  ctx: Context,
  parentSpan: Span,
  projectConfig: ProjectConfig,
  configPath: string,
  options: {
    typecheck: TypeCheckMode;
    typecheckComponents: boolean;
    adminKey: string;
    url: string;
    deploymentName: string | null;
    deploymentType?: DeploymentType;
    verbose: boolean;
    debugBundlePath?: string | undefined;
    dryRun: boolean;
    generateCommonJSApi?: boolean;
    debug: boolean;
    writePushRequest?: string | undefined;
    codegen: boolean;
    liveComponentSources?: boolean;
    pushAllModules?: boolean;
    debugNodeApis: boolean;
    largeIndexDeletionCheck: LargeIndexDeletionCheck;
    largeIndexBackfillCheck: LargeIndexBackfillCheck;
    stagedValidatorDeletionCheck: StagedValidatorDeletionCheck;
    warnOnSlowSchemaValidation: boolean;
    codegenOnlyThisComponent?: string | undefined;
  },
): Promise<StartPushResponse | null> {
  const convexDir = await getFunctionsDirectoryPath(ctx);

  // '.' means use the process current working directory, it's the default behavior.
  // Spelling it out here to be explicit for a future where this code can run
  // from other directories.
  // In esbuild the working directory is used to print error messages and resolving
  // relatives paths passed to it. It generally doesn't matter for resolving imports,
  // imports are resolved from the file where they are written.
  const absWorkingDir = path.resolve(".");
  const isComponent = isComponentDirectory(ctx, convexDir, true);
  if (isComponent.kind === "err") {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Invalid component root directory (${isComponent.why}): ${convexDir}`,
    });
  }

  let rootComponent = isComponent.component;
  if (options.codegenOnlyThisComponent) {
    const absolutePath = path.resolve(options.codegenOnlyThisComponent);
    let componentConfigPath: string;

    // Must be a directory containing a convex.config.ts
    componentConfigPath = path.join(absolutePath, DEFINITION_FILENAME_TS);
    if (!ctx.fs.exists(componentConfigPath)) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "invalid filesystem data",
        printedMessage: `Only directories with convex.config.ts files are supported, this directory does not: ${absolutePath}`,
      });
    }

    const syntheticConfigPath = path.join(
      rootComponent.path,
      DEFINITION_FILENAME_TS,
    );
    rootComponent = {
      isRoot: true,
      path: rootComponent.path,
      definitionPath: syntheticConfigPath,
      isRootWithoutConfig: false,
      syntheticComponentImport: componentConfigPath,
    };
  }

  changeSpinner("Finding component definitions...");
  // Create a list of relevant component directories. These are just for knowing
  // while directories to bundle in bundleDefinitions and bundleImplementations.
  // This produces a bundle in memory as a side effect but it's thrown away.
  const { components, dependencyGraph } = await parentSpan.enterAsync(
    "componentGraph",
    () =>
      componentGraph(
        ctx,
        absWorkingDir,
        rootComponent,
        !!options.liveComponentSources,
        options.verbose,
      ),
  );

  // Initial codegen is everything we need to get code runnable:
  // root components and other components need a basic _generated/api.ts etc.
  // just to make the code bundleable and runnable so we can analyze.
  if (options.codegen) {
    changeSpinner("Generating server code...");
    await parentSpan.enterAsync("doInitialComponentCodegen", () =>
      withTmpDir(async (tmpDir) => {
        // Skip the root in component cases
        if (!rootComponent.syntheticComponentImport) {
          // Do root first so if a component fails, we'll at least have a working root.
          await doInitialComponentCodegen(ctx, tmpDir, rootComponent, options);
        }
        for (const directory of components.values()) {
          if (directory.isRoot) {
            continue;
          }
          // When --component-dir is used, only generate code for the target component
          if (
            rootComponent.syntheticComponentImport &&
            directory.definitionPath !== rootComponent.syntheticComponentImport
          ) {
            continue;
          }
          await doInitialComponentCodegen(ctx, tmpDir, directory, options);
        }
      }),
    );
  }

  changeSpinner("Bundling component definitions...");
  // This bundles everything but the actual function definitions
  const {
    appDefinitionSpecWithoutImpls,
    componentDefinitionSpecsWithoutImpls,
  } = await parentSpan.enterAsync("bundleDefinitions", () =>
    bundleDefinitions(
      ctx,
      absWorkingDir,
      dependencyGraph,
      rootComponent,
      // Note that this *includes* the root component.
      [...components.values()],
      !!options.liveComponentSources,
      options.verbose,
    ),
  );

  if (options.debugNodeApis) {
    await debugIsolateEndpointBundles(ctx, projectConfig, configPath);
    logFinishedStep(
      "All non-'use node' entry points successfully bundled. Skipping rest of push.",
    );
    return null;
  }

  changeSpinner("Bundling component schemas and implementations...");
  const { appImplementation, componentImplementations } =
    await parentSpan.enterAsync("bundleImplementations", () =>
      bundleImplementations({
        ctx,
        rootComponentDirectory: rootComponent,
        // When running codegen for a specific component, don't bundle the root.
        componentDirectories: [...components.values()].filter(
          (dir) => !dir.isRoot && !dir.syntheticComponentImport,
        ),
        nodeExternalPackages: projectConfig.node.externalPackages,
        extraConditions: options.liveComponentSources
          ? ["@convex-dev/component-source"]
          : [],
        verbose: options.verbose,
        includeSourcesContent:
          projectConfig.bundler?.includeSourcesContent ?? false,
      }),
    );
  if (options.debugBundlePath) {
    const { config: localConfig } = await configFromProjectConfig(
      ctx,
      projectConfig,
      configPath,
      options.verbose,
    );
    // TODO(ENG-6972): Actually write the bundles for components.
    await handleDebugBundlePath(ctx, options.debugBundlePath, localConfig);
    logMessage(
      `Wrote bundle and metadata for modules in the root to ${options.debugBundlePath}. Skipping rest of push.`,
    );
    return null;
  }

  // We're just using the version this CLI is running with for now.
  // This could be different than the version of `convex` the app runs with
  // if the CLI is installed globally.
  // TODO: This should be the version of the `convex` package used by each
  // component, and may be different for each component.
  const udfServerVersion = version;

  const { unchangedModuleHashes, changedModules } = options.pushAllModules
    ? {
        unchangedModuleHashes: [],
        changedModules: appImplementation.functions,
      }
    : await parentSpan.enterAsync("getUnchangedModuleHashesFromServer", () =>
        getUnchangedModuleHashesFromServer(ctx, appImplementation, options),
      );

  const appDefinition: AppDefinitionConfig = {
    ...appDefinitionSpecWithoutImpls,
    schema: appImplementation.schema,
    changedModules,
    unchangedModuleHashes,
    udfServerVersion,
  };

  const componentDefinitions: ComponentDefinitionConfig[] = [];
  for (const componentDefinition of componentDefinitionSpecsWithoutImpls) {
    const impl = componentImplementations.filter(
      (impl) => impl.definitionPath === componentDefinition.definitionPath,
    )[0];
    if (!impl) {
      return await ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `missing! couldn't find ${componentDefinition.definitionPath} in ${componentImplementations.map((impl) => impl.definitionPath).toString()}`,
      });
    }
    componentDefinitions.push({
      ...componentDefinition,
      ...impl,
      udfServerVersion,
    });
  }
  const startPushRequest = {
    adminKey: options.adminKey,
    dryRun: options.dryRun,
    functions: projectConfig.functions,
    appDefinition,
    componentDefinitions,
    nodeDependencies: appImplementation.externalNodeDependencies,
    nodeVersion: projectConfig.node.nodeVersion,
    forCodegen: !!options.codegenOnlyThisComponent,
  };
  if (options.writePushRequest) {
    const pushRequestPath = path.resolve(options.writePushRequest);
    ctx.fs.writeUtf8File(
      `${pushRequestPath}.json`,
      JSON.stringify(startPushRequest),
    );
    return null;
  }
  logStartPushSizes(parentSpan, startPushRequest);

  const schemaEvaluation = sharedSchemaEvaluation(
    ctx,
    startPushRequest,
    options,
  );
  if (options.largeIndexDeletionCheck !== "no verification") {
    await parentSpan.enterAsync("checkForLargeIndexDeletion", (span) =>
      checkForLargeIndexDeletion({
        ctx,
        span,
        request: startPushRequest,
        options,
        schemaEvaluation,
        askForConfirmation:
          options.largeIndexDeletionCheck === "ask for confirmation",
      }),
    );
  }

  const largeIndexBackfillCheck = options.largeIndexBackfillCheck;
  if (largeIndexBackfillCheck !== "no verification") {
    await parentSpan.enterAsync("checkForLargeIndexBackfill", (span) =>
      checkForLargeIndexBackfill({
        ctx,
        span,
        schemaEvaluation,
        options,
        // A dry run never blocks on a backfill, so it only reports.
        mode: options.dryRun ? "warn" : largeIndexBackfillCheck,
      }),
    );
  }

  const stagedValidatorDeletionCheck = options.stagedValidatorDeletionCheck;
  if (stagedValidatorDeletionCheck !== "no verification") {
    await parentSpan.enterAsync(
      "checkForStagedValidatorDeletion",
      async (span) => {
        const response = await schemaEvaluation(span);
        await checkForStagedValidatorDeletion({
          ctx,
          response,
          askForConfirmation:
            stagedValidatorDeletionCheck === "ask for confirmation",
          // A dry run previews the confirmation required by a real push.
          dryRun: options.dryRun,
        });
      },
    );
  }

  // A real push evaluates the schema only when the evaluation is already
  // paid for by the unstaging check or the pushed schema stages a validator,
  // so `dev` pushes of ordinary schemas skip the isolate evaluation and round
  // trip that the status report would otherwise add on every save.
  if (
    options.warnOnSlowSchemaValidation &&
    (options.dryRun ||
      stagedValidatorDeletionCheck !== "no verification" ||
      pushStagesValidators(startPushRequest))
  ) {
    await parentSpan.enterAsync("checkForSlowSchemaValidation", (span) =>
      checkForSlowSchemaValidation({
        span,
        schemaEvaluation,
        dryRun: options.dryRun,
      }),
    );
  }

  changeSpinner("Uploading functions to Convex...");
  const startPushResponse = await parentSpan.enterAsync("startPush", (span) =>
    startPush(ctx, span, startPushRequest, options),
  );

  if (options.verbose) {
    logMessage(
      "startPush: " +
        JSON.stringify(startPushResponseForLogging(startPushResponse), null, 2),
    );
  }

  if (options.codegen) {
    changeSpinner("Generating TypeScript bindings...");
    await parentSpan.enterAsync("doFinalComponentCodegen", () =>
      withTmpDir(async (tmpDir) => {
        // TODO generating code for the root component last might be better DX
        // When running codegen for a specific component, don't generate types for the root
        if (!rootComponent.syntheticComponentImport) {
          // Do the root first
          await doFinalComponentCodegen(
            ctx,
            tmpDir,
            rootComponent,
            rootComponent,
            startPushResponse,
            components,
            options,
          );
        }
        for (const directory of components.values()) {
          if (directory.isRoot) {
            continue;
          }
          // When --component-dir is used, only generate code for the target component
          if (
            rootComponent.syntheticComponentImport &&
            directory.definitionPath !== rootComponent.syntheticComponentImport
          ) {
            continue;
          }
          await doFinalComponentCodegen(
            ctx,
            tmpDir,
            rootComponent,
            directory,
            startPushResponse,
            components,
            options,
          );
        }
      }),
    );
  }

  changeSpinner("Running TypeScript...");
  await parentSpan.enterAsync("typeCheckFunctionsInMode", async () => {
    // When running codegen for a specific component, don't typecheck the root
    if (!rootComponent.syntheticComponentImport) {
      await typeCheckFunctionsInMode(
        ctx,
        options.typecheck,
        rootComponent.path,
      );
    }
    if (options.typecheckComponents) {
      for (const directory of components.values()) {
        if (!directory.isRoot) {
          await typeCheckFunctionsInMode(
            ctx,
            options.typecheck,
            directory.path,
          );
        }
      }
    } else if (rootComponent.syntheticComponentImport) {
      // When running codegen for a specific component, only typecheck that component.
      for (const directory of components.values()) {
        if (
          directory.isRoot ||
          directory.definitionPath !== rootComponent.syntheticComponentImport
        ) {
          continue;
        }
        await typeCheckFunctionsInMode(ctx, options.typecheck, directory.path);
      }
    }
  });

  return startPushResponse;
}

function logStartPushSizes(span: Span, startPushRequest: StartPushRequest) {
  let v8Size = 0;
  let v8Count = 0;
  let nodeSize = 0;
  let nodeCount = 0;

  for (const componentDefinition of startPushRequest.componentDefinitions) {
    for (const module of componentDefinition.functions) {
      if (module.environment === "isolate") {
        v8Size += module.source.length + (module.sourceMap ?? "").length;
        v8Count += 1;
      } else if (module.environment === "node") {
        nodeSize += module.source.length + (module.sourceMap ?? "").length;
        nodeCount += 1;
      }
    }
  }
  span.setProperty("v8_size", v8Size.toString());
  span.setProperty("v8_count", v8Count.toString());
  span.setProperty("node_size", nodeSize.toString());
  span.setProperty("node_count", nodeCount.toString());
}

export async function runComponentsPush(
  ctx: Context,
  options: PushOptions,
  configPath: string,
  projectConfig: ProjectConfig,
) {
  const reporter = new Reporter();
  const pushSpan = Span.root(reporter, "runComponentsPush");
  pushSpan.setProperty("cli_version", version);
  const verbose = options.verbose || options.dryRun;

  await ensureHasConvexDependency(ctx, "push");

  const startPushResponse = await pushSpan.enterAsync(
    "startComponentsPushAndCodegen",
    (span) =>
      startComponentsPushAndCodegen(
        ctx,
        span,
        projectConfig,
        configPath,
        options,
      ),
  );
  if (!startPushResponse) {
    return;
  }

  await pushSpan.enterAsync("waitForSchema", (span) =>
    waitForSchema(ctx, span, startPushResponse, options),
  );

  if (verbose) {
    const remoteConfigWithModuleHashes = await pullConfig(
      ctx,
      undefined,
      undefined,
      options.url,
      options.adminKey,
    );

    const { config: localConfig } = await configFromProjectConfig(
      ctx,
      projectConfig,
      configPath,
      options.verbose,
    );

    changeSpinner("Diffing local code and deployment state...");
    const { diffString } = diffConfig(
      remoteConfigWithModuleHashes,
      localConfig,
    );

    logFinishedStep(
      `Remote config ${
        options.dryRun ? "would" : "will"
      } be overwritten with the following changes:\n  ` +
        diffString.replace(/\n/g, "\n  "),
    );
  }

  const finishPushResponse = await pushSpan.enterAsync("finishPush", (span) =>
    finishPush(ctx, span, startPushResponse, options),
  );
  printDiff(startPushResponse, finishPushResponse, options);
  pushSpan.end();

  // Asynchronously report that the push completed.
  if (!options.dryRun) {
    void reportPushCompleted(ctx, options.adminKey, options.url, reporter);
  }
}

function printDiff(
  startPushResponse: StartPushResponse,
  finishPushResponse: FinishPushDiff,
  opts: { verbose: boolean; dryRun: boolean; deploymentName: string | null },
) {
  if (opts.verbose) {
    const diffString = JSON.stringify(
      redactEnvironmentVariableValues(
        finishPushResponse,
        startPushResponse.environmentVariables,
      ),
      null,
      2,
    );
    logMessage(diffString);
    return;
  }
  const indexDiffs = startPushResponse.schemaChange.indexDiffs;
  const { componentDiffs } = finishPushResponse;

  // Print out index diffs for the root component.
  let rootDiff = indexDiffs?.[""] || componentDiffs[""]?.indexDiff;
  if (rootDiff) {
    if (rootDiff.removed_indexes.length > 0) {
      let msg = `${opts.dryRun ? "Would delete" : "Deleted"} table indexes:\n`;
      for (const index of rootDiff.removed_indexes) {
        msg += `  [-] ${formatIndex(index)}\n`;
      }
      msg = msg.slice(0, -1); // strip last new line
      logFinishedStep(msg);
    }

    const addedEnabled = rootDiff.added_indexes.filter((i) => !i.staged);
    if (addedEnabled.length > 0) {
      let msg = `${opts.dryRun ? "Would add" : "Added"} table indexes:\n`;
      for (const index of addedEnabled) {
        msg += `  [+] ${formatIndex(index)}\n`;
      }
      msg = msg.slice(0, -1); // strip last new line
      logFinishedStep(msg);
    }

    const addedStaged = rootDiff.added_indexes.filter((i) => i.staged);
    if (addedStaged.length > 0) {
      let msg = `${opts.dryRun ? "Would add" : "Added"} staged table indexes:\n`;
      for (const index of addedStaged) {
        const table = index.name.split(".")[0];
        const progressLink = deploymentDashboardUrlPage(
          opts.deploymentName,
          `/data?table=${table}&showIndexes=true`,
        );
        msg += `  [+] ${formatIndex(index)}\n`;
        msg += `      See progress: ${progressLink}\n`;
      }
      msg = msg.slice(0, -1); // strip last new line
      logFinishedStep(msg);
    }

    if (rootDiff.enabled_indexes && rootDiff.enabled_indexes.length > 0) {
      let msg = opts.dryRun
        ? `These indexes would be enabled:\n`
        : `These indexes are now enabled:\n`;
      for (const index of rootDiff.enabled_indexes) {
        msg += `  [*] ${formatIndex(index)}\n`;
      }
      msg = msg.slice(0, -1); // strip last new line
      logFinishedStep(msg);
    }

    if (rootDiff.disabled_indexes && rootDiff.disabled_indexes.length > 0) {
      let msg = opts.dryRun
        ? `These indexes would be staged:\n`
        : `These indexes are now staged:\n`;
      for (const index of rootDiff.disabled_indexes) {
        msg += `  [*] ${formatIndex(index)}\n`;
      }
      msg = msg.slice(0, -1); // strip last new line
      logFinishedStep(msg);
    }
  }

  // Only show component level diffs for other components.
  for (const [componentPath, componentDiff] of Object.entries(componentDiffs)) {
    if (componentPath === "") {
      continue;
    }
    if (componentDiff.diffType.type === "create") {
      logFinishedStep(`Installed component ${componentPath}.`);
    }
    if (componentDiff.diffType.type === "unmount") {
      logFinishedStep(`Unmounted component ${componentPath}.`);
    }
    if (componentDiff.diffType.type === "remount") {
      logFinishedStep(`Remounted component ${componentPath}.`);
    }
  }
}
