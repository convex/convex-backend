import { chalkStderr } from "chalk";
import path from "path";
// eslint-disable-next-line no-restricted-imports
import { promises as nodeFs } from "fs";
import { z } from "zod";
import { Context } from "../../bundler/context.js";
import { TypescriptCompiler } from "./typecheck.js";
import {
  changeSpinner,
  logError,
  logFailure,
  logFinishedStep,
  logMessage,
  logWarning,
  showSpinner,
} from "../../bundler/log.js";
import {
  Bundle,
  BundleHash,
  bundle,
  bundleAuthConfig,
  entryPointsByEnvironment,
} from "../../bundler/index.js";
import { version } from "../version.js";
import { deploymentDashboardUrlPage } from "./dashboard.js";
import {
  functionsDir,
  ErrorData,
  loadPackageJson,
  deploymentFetch,
  deprecationCheckWarning,
  logAndHandleFetchError,
  ThrowingFetchError,
  currentPackageHomepage,
} from "./utils/utils.js";
import { recursivelyDelete } from "./fsUtils.js";
import { NodeDependency } from "./deployApi/modules.js";
import { ComponentDefinitionPath } from "./components/definition/directoryStructure.js";
import {
  LocalDeploymentError,
  printLocalDeploymentOnError,
} from "./localDeployment/errors.js";
import { debugIsolateBundlesSerially } from "../../bundler/debugBundle.js";
import { DeploymentType } from "./api.js";
import { isAnonymousDeployment } from "./deployment.js";
export { productionProvisionHost, provisionHost } from "./utils/utils.js";

/** Type representing WorkOS AuthKit integration configuration. */
export interface AuthKitConfigureSettings {
  redirectUris?: string[];
  appHomepageUrl?: string;
  corsOrigins?: string[];
}

export interface AuthKitEnvironmentConfig {
  environmentType?: "development" | "staging" | "production";
  configure?: false | AuthKitConfigureSettings;
  localEnvVars?: false | Record<string, string>;
}

export interface AuthKitConfig {
  dev?: AuthKitEnvironmentConfig;
  preview?: AuthKitEnvironmentConfig;
  prod?: AuthKitEnvironmentConfig;
}
/**
 * convex.json file parsing notes
 *
 * - Unknown fields at the top level and in node and codegen are preserved
 *   so that older CLI versions can deploy new projects (this functionality
 *   will be removed in the future).
 * - convex.json does not allow comments, but this could change in the future.
 *   Previously it contained automatically set values like productionUrl
 *   but it's more like a config file now.
 */

/** Type representing Convex project configuration. */
export interface ProjectConfig {
  // ⚠️ When updating this, please also update the file used by IDEs for autocompletion and validation:
  // -> npm-packages/convex/schemas/convex.schema.json

  functions: string;
  node: {
    externalPackages: string[];
    // nodeVersion has no default value, its presence/absence is meaningful
    nodeVersion?: string | undefined;
  };
  generateCommonJSApi: boolean;

  codegen: {
    staticApi: boolean;
    staticDataModel: boolean;
    legacyComponentApi?: boolean;
    fileType?: "ts" | "js/dts";
  };

  bundler?: {
    includeSourcesContent?: boolean;
  };

  typescriptCompiler?: TypescriptCompiler;

  // WorkOS AuthKit integration configuration
  authKit?: AuthKitConfig | undefined;

  // Convex AI files user preferences.
  aiFiles?: {
    // When false, disables all AI files prompts and staleness messages.
    enabled?: boolean;
    // @deprecated use `enabled` instead.
    disableStalenessMessage?: boolean;
    // Configuration for agent skills installed by Convex.
    skills?: {
      // List of agents to install skills for (e.g. 'claude-code', 'codex', 'cursor').
      // Defaults to ['claude-code', 'codex'].
      agents?: string[];
    };
  };
}

export interface Config {
  projectConfig: ProjectConfig;
  modules: Bundle[];
  nodeDependencies: NodeDependency[];
  schemaId?: string;
  udfServerVersion?: string;
  nodeVersion?: string | undefined;
}

export interface ConfigWithModuleHashes {
  projectConfig: ProjectConfig;
  moduleHashes: BundleHash[];
  nodeDependencies: NodeDependency[];
  schemaId?: string;
  udfServerVersion?: string;
}

const DEFAULT_FUNCTIONS_PATH = "convex/";

/** Whether .ts file extensions should be used for generated code (default is false). */
export function usesTypeScriptCodegen(projectConfig: ProjectConfig): boolean {
  return projectConfig.codegen.fileType === "ts";
}

/** Whether the new component API import style should be used */
export function usesComponentApiImports(projectConfig: ProjectConfig): boolean {
  return projectConfig.codegen.legacyComponentApi !== true;
}

/**
 * Get the authKit configuration from convex.json.
 */
export async function getAuthKitConfig(
  ctx: Context,
  projectConfig: ProjectConfig,
): Promise<AuthKitConfig | undefined> {
  // If there's an explicit authKit config, use it
  if ("authKit" in projectConfig) {
    return projectConfig.authKit;
  }

  // TODO remove this after a few versions
  // Migration help: is this one of the hardcoded templates that has special
  // behavior without a convex.json? Encourage them to upgrade the template.
  const homepage = await currentPackageHomepage(ctx);
  const isOldWorkOSTemplate = !!(
    homepage &&
    [
      "https://github.com/workos/template-convex-nextjs-authkit/#readme",
      "https://github.com/workos/template-convex-react-vite-authkit/#readme",
      "https://github.com:workos/template-convex-react-vite-authkit/#readme",
      "https://github.com/workos/template-convex-tanstack-start-authkit/#readme",
    ].includes(homepage)
  );

  if (isOldWorkOSTemplate) {
    logWarning(
      "The template this project is based on has been updated to work with this version of Convex.",
    );
    logWarning(
      "Please copy the convex.json from the latest template version or add an 'authKit' section.",
    );
    logMessage("Learn more at https://docs.convex.dev/auth/authkit");
  }
}

export async function getAuthKitEnvironmentConfig(
  ctx: Context,
  projectConfig: ProjectConfig,
  deploymentType: "dev" | "preview" | "prod",
): Promise<AuthKitEnvironmentConfig | undefined> {
  const authKitConfig = await getAuthKitConfig(ctx, projectConfig);
  return authKitConfig?.[deploymentType];
}

/** Error parsing ProjectConfig representation. */
class ParseError extends Error {}

// WorkOS AuthKit configuration schemas
const AuthKitConfigureSchema = z.union([
  z.literal(false),
  z.object({
    redirectUris: z.array(z.string()).optional(),
    appHomepageUrl: z.string().optional(),
    corsOrigins: z.array(z.string()).optional(),
  }),
]);

const AuthKitLocalEnvVarsSchema = z.union([
  z.literal(false),
  z.record(z.string()),
]);

const AuthKitEnvironmentConfigSchema = z.object({
  environmentType: z.enum(["development", "staging", "production"]).optional(),
  configure: AuthKitConfigureSchema.optional(),
  localEnvVars: AuthKitLocalEnvVarsSchema.optional(),
});

const AuthKitConfigSchema = z
  .object({
    dev: AuthKitEnvironmentConfigSchema.optional(),
    preview: AuthKitEnvironmentConfigSchema.optional(),
    prod: AuthKitEnvironmentConfigSchema.optional(),
  })
  .refine(
    (data) => {
      // Validation: environmentType only allowed in prod
      const devEnvType = data.dev?.environmentType;
      const previewEnvType = data.preview?.environmentType;
      if (devEnvType || previewEnvType) {
        return false;
      }
      return true;
    },
    {
      message: "authKit.environmentType is only allowed in the prod section",
      path: ["environmentType"],
    },
  )
  .refine(
    (data) => {
      // Validation: localEnvVars only allowed for dev
      // Check preview doesn't have localEnvVars
      if (
        data.preview?.localEnvVars !== undefined &&
        data.preview?.localEnvVars !== false
      ) {
        return false;
      }
      // Check prod doesn't have localEnvVars
      if (
        data.prod?.localEnvVars !== undefined &&
        data.prod?.localEnvVars !== false
      ) {
        return false;
      }
      return true;
    },
    {
      message:
        "authKit.localEnvVars is only supported for dev deployments. Preview and prod deployments must configure environment variables directly in the deployment platform.",
      path: ["localEnvVars"],
    },
  );
// Separate Node and Codegen schemas so we can parse these loose or strict
const NodeSchema = z.object({
  externalPackages: z
    .array(z.string())
    .default([])
    .describe(
      "list of npm packages to install at deploy time instead of bundling. Packages with binaries should be added here.",
    ),
  nodeVersion: z
    .string()
    .optional()
    .describe("The Node.js version to use for Node.js functions"),
});

const CodegenSchema = z.object({
  staticApi: z
    .boolean()
    .default(false)
    .describe(
      "Use Convex function argument validators and return value validators to generate a typed API object",
    ),
  staticDataModel: z.boolean().default(false),
  // These optional fields have no defaults - their presence/absence is meaningful
  legacyComponentApi: z.boolean().optional(),
  fileType: z.enum(["ts", "js/dts"]).optional(),
});

const BundlerSchema = z.object({
  includeSourcesContent: z
    .boolean()
    .default(false)
    .describe(
      "Whether to include original source code in source maps. Set to false to reduce bundle size.",
    ),
});

const AiFilesSchema = z.object({
  enabled: z.boolean().optional(),
  disableStalenessMessage: z.boolean().optional(),
  skills: z
    .object({
      agents: z.array(z.string()).optional(),
    })
    .optional(),
});

const refineToObject = <T extends z.ZodTypeAny>(schema: T) =>
  schema.refine((val) => val !== null && !Array.isArray(val), {
    message: "Expected `convex.json` to contain an object",
  });

// Factory function to create schema with strict or passthrough behavior
const createProjectConfigSchema = (strict: boolean) => {
  const nodeSchema = strict ? NodeSchema.strict() : NodeSchema.passthrough();
  const codegenSchema = strict
    ? CodegenSchema.strict()
    : CodegenSchema.passthrough();
  const bundlerSchema = strict
    ? BundlerSchema.strict()
    : BundlerSchema.passthrough();

  const baseObject = z.object({
    functions: z
      .string()
      .default(DEFAULT_FUNCTIONS_PATH)
      .describe("Relative file path to the convex directory"),
    node: nodeSchema.default({ externalPackages: [] }),
    codegen: codegenSchema.default({
      staticApi: false,
      staticDataModel: false,
    }),
    bundler: bundlerSchema.default({ includeSourcesContent: false }).optional(),
    generateCommonJSApi: z.boolean().default(false),
    typescriptCompiler: z
      .enum(["tsc", "tsgo"])
      .optional()
      .describe(
        "TypeScript compiler to use for typechecking (`@typescript/native-preview` must be installed to use `tsgo`)",
      ),

    // Optional $schema field for JSON schema validation in editors
    $schema: z.string().optional(),
    // WorkOS AuthKit integration configuration
    authKit: AuthKitConfigSchema.optional(),
    aiFiles: AiFilesSchema.optional(),

    // Deprecated fields that have been deprecated for years, only here so we
    // know it's safe to delete them.
    project: z.string().optional(),
    team: z.string().optional(),
    prodUrl: z.string().optional(),
  });

  // Apply strict or passthrough BEFORE refine
  const withStrictness = strict
    ? baseObject.strict()
    : baseObject.passthrough();

  // Now apply the refinement
  return withStrictness.refine(
    (data) => {
      // Validate that generateCommonJSApi is not true when using TypeScript codegen
      if (data.generateCommonJSApi && data.codegen.fileType === "ts") {
        return false;
      }
      return true;
    },
    {
      message:
        'Cannot use `generateCommonJSApi: true` with `codegen.fileType: "ts"`. ' +
        "CommonJS modules require JavaScript generation. " +
        'Either set `codegen.fileType: "js/dts"` or remove `generateCommonJSApi`.',
      path: ["generateCommonJSApi"],
    },
  );
};

// Parse allowing extra fields (for forward compatibility)
const ProjectConfigSchema = refineToObject(createProjectConfigSchema(false));

// Strict schema warn about extra keys
const ProjectConfigSchemaStrict = refineToObject(
  createProjectConfigSchema(true),
);

const warnedUnknownKeys = new Set<string>();
export function resetUnknownKeyWarnings() {
  warnedUnknownKeys.clear();
}

/** Parse object to ProjectConfig. */
export async function parseProjectConfig(
  ctx: Context,
  obj: any,
): Promise<ProjectConfig> {
  if (typeof obj !== "object" || obj === null || Array.isArray(obj)) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: "Expected `convex.json` to contain an object",
    });
  }

  try {
    // Try strict parse first to detect unknown keys
    return ProjectConfigSchemaStrict.parse(obj);
  } catch (error) {
    if (error instanceof z.ZodError) {
      // Check if all issues are unrecognized_keys issues
      const unknownKeyIssues = error.issues.filter(
        (issue) => issue.code === "unrecognized_keys",
      );

      if (
        unknownKeyIssues.length > 0 &&
        unknownKeyIssues.length === error.issues.length
      ) {
        // All errors are just unknown keys - warn about them
        for (const issue of unknownKeyIssues) {
          if (issue.code === "unrecognized_keys") {
            const pathPrefix =
              issue.path.length > 0 ? issue.path.join(".") + "." : "";
            const unknownKeys = issue.keys as string[];
            const newUnknownKeys = unknownKeys.filter(
              (key) => !warnedUnknownKeys.has(pathPrefix + key),
            );

            if (newUnknownKeys.length > 0) {
              const fullPath =
                issue.path.length > 0
                  ? `\`${issue.path.join(".")}\``
                  : "`convex.json`";
              logMessage(
                chalkStderr.yellow(
                  `Warning: Unknown ${newUnknownKeys.length === 1 ? "property" : "properties"} in ${fullPath}: ${newUnknownKeys.map((k) => `\`${k}\``).join(", ")}`,
                ),
              );
              logMessage(
                chalkStderr.gray(
                  "  These properties will be preserved but are not recognized by this version of Convex.",
                ),
              );

              // Track that we've warned about these keys
              newUnknownKeys.forEach((key) =>
                warnedUnknownKeys.add(pathPrefix + key),
              );
            }
          }
        }
        // Re-parse with passthrough schema to preserve unknown keys
        return ProjectConfigSchema.parse(obj);
      }

      // Handle validation errors we won't ignore
      if (error instanceof z.ZodError) {
        const issue = error.issues[0];
        const pathStr = issue.path.join(".");
        const message = pathStr
          ? `\`${pathStr}\` in \`convex.json\`: ${issue.message}`
          : `\`convex.json\`: ${issue.message}`;
        return await ctx.crash({
          exitCode: 1,
          errorType: "invalid filesystem data",
          printedMessage: message,
        });
      }
    }
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: (error as any).toString(),
    });
  }
}

// Parse a deployment config returned by the backend, picking out
// the fields we care about.
function parseBackendConfig(obj: any): {
  functions: string;
  nodeVersion?: string;
} {
  function throwParseError(message: string) {
    // Unexpected error
    // eslint-disable-next-line no-restricted-syntax
    throw new ParseError(message);
  }
  if (typeof obj !== "object") {
    throwParseError("Expected an object");
  }
  const { functions, nodeVersion } = obj;
  if (typeof functions !== "string") {
    throwParseError("Expected functions to be a string");
  }

  if (typeof nodeVersion !== "undefined" && typeof nodeVersion !== "string") {
    throwParseError("Expected nodeVersion to be a string");
  }

  return {
    functions,
    ...((nodeVersion ?? null) !== null ? { nodeVersion: nodeVersion } : {}),
  };
}

export function configName(): string {
  return "convex.json";
}

export async function configFilepath(ctx: Context): Promise<string> {
  const configFn = configName();
  // We used to allow src/convex.json, but no longer (as of 10/7/2022).
  // Leave an error message around to help people out. We can remove this
  // error message after a couple months.
  const preferredLocation = configFn;
  const wrongLocation = path.join("src", configFn);

  // Allow either location, but not both.
  const preferredLocationExists = ctx.fs.exists(preferredLocation);
  const wrongLocationExists = ctx.fs.exists(wrongLocation);
  if (preferredLocationExists && wrongLocationExists) {
    const message = `${chalkStderr.red(`Error: both ${preferredLocation} and ${wrongLocation} files exist!`)}\nConsolidate these and remove ${wrongLocation}.`;
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: message,
    });
  }
  if (!preferredLocationExists && wrongLocationExists) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      printedMessage: `Error: Please move ${wrongLocation} to the root of your project`,
    });
  }

  return preferredLocation;
}

export async function getFunctionsDirectoryPath(ctx: Context): Promise<string> {
  const { projectConfig, configPath } = await readProjectConfig(ctx);
  return functionsDir(configPath, projectConfig);
}

/** Read configuration from a local `convex.json` file. */
export async function readProjectConfig(ctx: Context): Promise<{
  projectConfig: ProjectConfig;
  configPath: string;
}> {
  if (!ctx.fs.exists("convex.json")) {
    // create-react-app bans imports from outside of src, so we can just
    // put the functions directory inside of src/ to work around this issue.
    const packages = await loadPackageJson(ctx);
    const isCreateReactApp = "react-scripts" in packages;
    return {
      projectConfig: {
        functions: isCreateReactApp
          ? `src/${DEFAULT_FUNCTIONS_PATH}`
          : DEFAULT_FUNCTIONS_PATH,
        node: {
          externalPackages: [],
        },
        generateCommonJSApi: false,
        codegen: {
          staticApi: false,
          staticDataModel: false,
        },
        aiFiles: {},
      },
      configPath: configName(),
    };
  }
  let projectConfig;
  const configPath = await configFilepath(ctx);
  try {
    projectConfig = await parseProjectConfig(
      ctx,
      JSON.parse(ctx.fs.readUtf8File(configPath)),
    );
  } catch (err) {
    if (err instanceof ParseError || err instanceof SyntaxError) {
      logError(chalkStderr.red(`Error: Parsing "${configPath}" failed`));
      logMessage(chalkStderr.gray(err.toString()));
    } else {
      logFailure(
        `Error: Unable to read project config file "${configPath}"\n` +
          "  Are you running this command from the root directory of a Convex project? If so, run `npx convex dev` first.",
      );
      if (err instanceof Error) {
        logError(chalkStderr.red(err.message));
      }
    }
    return await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem data",
      errForSentry: err,
      // TODO -- move the logging above in here
      printedMessage: null,
    });
  }
  return {
    projectConfig,
    configPath,
  };
}

/**
 * Given a {@link ProjectConfig}, add in the bundled modules to produce the
 * complete config.
 */
export async function configFromProjectConfig(
  ctx: Context,
  projectConfig: ProjectConfig,
  configPath: string,
  verbose: boolean,
): Promise<{
  config: Config;
  bundledModuleInfos: BundledModuleInfo[];
}> {
  const baseDir = functionsDir(configPath, projectConfig);
  // We bundle Node.js and Convex JS runtime functions entry points separately
  // since they execute on different platforms.
  const entryPoints = await entryPointsByEnvironment(ctx, baseDir);
  // es-build prints errors to console which would clobber our spinner.
  if (verbose) {
    showSpinner("Bundling modules for Convex's runtime...");
  }
  const convexResult = await bundle({
    ctx,
    dir: baseDir,
    entryPoints: entryPoints.isolate,
    generateSourceMaps: true,
    platform: "browser",
  });
  if (verbose) {
    logMessage(
      "Convex's runtime modules: ",
      convexResult.modules.map((m) => m.path),
    );
  }

  // Bundle node modules.
  if (verbose && entryPoints.node.length !== 0) {
    showSpinner("Bundling modules for Node.js runtime...");
  }
  const nodeResult = await bundle({
    ctx,
    dir: baseDir,
    entryPoints: entryPoints.node,
    generateSourceMaps: true,
    platform: "node",
    chunksFolder: path.join("_deps", "node"),
    externalPackagesAllowList: projectConfig.node.externalPackages,
  });
  if (verbose && entryPoints.node.length !== 0) {
    logMessage(
      "Node.js runtime modules: ",
      nodeResult.modules.map((m) => m.path),
    );
    if (projectConfig.node.externalPackages.length > 0) {
      logMessage(
        "Node.js runtime external dependencies (to be installed on the server): ",
        [...nodeResult.externalDependencies.entries()].map(
          (a) => `${a[0]}: ${a[1]}`,
        ),
      );
    }
  }
  const modules = convexResult.modules;
  modules.push(...nodeResult.modules);
  modules.push(...(await bundleAuthConfig(ctx, baseDir)).modules);

  const nodeDependencies: NodeDependency[] = [];
  for (const [moduleName, moduleVersion] of nodeResult.externalDependencies) {
    nodeDependencies.push({ name: moduleName, version: moduleVersion });
  }

  const bundledModuleInfos: BundledModuleInfo[] = Array.from(
    convexResult.bundledModuleNames.keys(),
  ).map((moduleName) => {
    return {
      name: moduleName,
      platform: "convex",
    };
  });
  bundledModuleInfos.push(
    ...Array.from(nodeResult.bundledModuleNames.keys()).map(
      (moduleName): BundledModuleInfo => {
        return {
          name: moduleName,
          platform: "node",
        };
      },
    ),
  );

  return {
    config: {
      projectConfig: projectConfig,
      modules: modules,
      nodeDependencies: nodeDependencies,
      // We're just using the version this CLI is running with for now.
      // This could be different than the version of `convex` the app runs with
      // if the CLI is installed globally.
      udfServerVersion: version,
      nodeVersion: projectConfig.node.nodeVersion,
    },
    bundledModuleInfos,
  };
}

/**
 * Bundle modules one by one for good bundler errors.
 */
export async function debugIsolateEndpointBundles(
  ctx: Context,
  projectConfig: ProjectConfig,
  configPath: string,
): Promise<void> {
  const baseDir = functionsDir(configPath, projectConfig);
  const entryPoints = await entryPointsByEnvironment(ctx, baseDir);
  if (entryPoints.isolate.length === 0) {
    logFinishedStep("No non-'use node' modules found.");
  }
  await debugIsolateBundlesSerially(ctx, {
    entryPoints: entryPoints.isolate,
    extraConditions: [],
    dir: baseDir,
  });
}

/**
 * Read the config from `convex.json` and bundle all the modules.
 */
export async function readConfig(
  ctx: Context,
  verbose: boolean,
): Promise<{
  config: Config;
  configPath: string;
  bundledModuleInfos: BundledModuleInfo[];
}> {
  const { projectConfig, configPath } = await readProjectConfig(ctx);
  const { config, bundledModuleInfos } = await configFromProjectConfig(
    ctx,
    projectConfig,
    configPath,
    verbose,
  );
  return { config, configPath, bundledModuleInfos };
}

/**
 * Ensure the functions directory exists.
 *
 * Note: convex.json is treated as user-owned and is not modified by the CLI.
 * Use writeAiFilesConfig() to explicitly patch the aiFiles section.
 */
export async function ensureConvexFunctionsDir(
  ctx: Context,
  projectConfig: ProjectConfig,
) {
  const configPath = await configFilepath(ctx);
  ctx.fs.mkdir(functionsDir(configPath, projectConfig), {
    allowExisting: true,
  });
}

export function removedExistingConfig(
  ctx: Context,
  configPath: string,
  options: { allowExistingConfig?: boolean },
) {
  if (!options.allowExistingConfig) {
    return false;
  }
  recursivelyDelete(ctx, configPath);
  logFinishedStep(`Removed existing ${configPath}`);
  return true;
}

/** Pull configuration for the root app component from the given remote origin. */
export async function pullConfig(
  ctx: Context,
  project: string | undefined,
  team: string | undefined,
  origin: string,
  adminKey: string,
): Promise<ConfigWithModuleHashes> {
  const fetch = deploymentFetch(ctx, {
    deploymentUrl: origin,
    adminKey,
  });

  changeSpinner("Downloading current deployment state...");
  try {
    const res = await fetch("/api/get_config_hashes", {
      method: "POST",
      body: JSON.stringify({ version, adminKey }),
    });
    deprecationCheckWarning(ctx, res);
    const data = await res.json();
    const backendConfig = parseBackendConfig(data.config);
    const projectConfig = {
      ...backendConfig,
      node: {
        // This field is not stored in the backend, which is ok since it is also
        // not used to diff configs.
        externalPackages: [],
        nodeVersion: data.nodeVersion,
      },
      // This field is not stored in the backend, it only affects the client.
      generateCommonJSApi: false,
      // This field is also not stored in the backend, it only affects the client.
      codegen: {
        staticApi: false,
        staticDataModel: false,
      },
      project,
      team,
      prodUrl: origin,
    };
    return {
      projectConfig,
      moduleHashes: data.moduleHashes,
      // TODO(presley): Add this to diffConfig().
      nodeDependencies: data.nodeDependencies,
      udfServerVersion: data.udfServerVersion,
    };
  } catch (err: unknown) {
    logFailure(`Error: Unable to pull deployment config from ${origin}`);
    return await logAndHandleFetchError(ctx, err);
  }
}

interface BundledModuleInfo {
  name: string;
  platform: "node" | "convex";
}

/**
 * A component definition spec contains enough information to create bundles
 * of code that must be analyzed in order to construct a ComponentDefinition.
 *
 * Most paths are relative to the directory of the definitionPath.
 */
export type ComponentDefinitionSpec = {
  /** This path is relative to the app (root component) directory. */
  definitionPath: ComponentDefinitionPath;

  /** Dependencies are paths to the directory of the dependency component definition from the app (root component) directory */
  dependencies: ComponentDefinitionPath[];

  // All other paths are relative to the directory of the definitionPath above.
  definition: Bundle;
  schema: Bundle;
  functions: Bundle[];
};

export type AppDefinitionSpec = Omit<
  ComponentDefinitionSpec,
  "definitionPath"
> & {
  // Only app (root) component specs contain an auth bundle.
  auth: Bundle | null;
};

export type ComponentDefinitionSpecWithoutImpls = Omit<
  ComponentDefinitionSpec,
  "schema" | "functions"
>;
export type AppDefinitionSpecWithoutImpls = Omit<
  AppDefinitionSpec,
  "schema" | "functions" | "auth"
>;

/** Generate a human-readable diff between the two configs. */
export function diffConfig(
  oldConfig: ConfigWithModuleHashes,
  newConfig: Config,
): { diffString: string } {
  let diff = "";

  let versionMessage = "";
  const matches = oldConfig.udfServerVersion === newConfig.udfServerVersion;
  if (oldConfig.udfServerVersion && (!newConfig.udfServerVersion || !matches)) {
    versionMessage += `[-] ${oldConfig.udfServerVersion}\n`;
  }
  if (newConfig.udfServerVersion && (!oldConfig.udfServerVersion || !matches)) {
    versionMessage += `[+] ${newConfig.udfServerVersion}\n`;
  }
  if (versionMessage) {
    diff += "Change the server's function version:\n";
    diff += versionMessage;
  }

  if (oldConfig.projectConfig.node.nodeVersion !== newConfig.nodeVersion) {
    diff += "Change the server's version for Node.js actions:\n";
    if (oldConfig.projectConfig.node.nodeVersion) {
      diff += `[-] ${oldConfig.projectConfig.node.nodeVersion}\n`;
    }
    if (newConfig.nodeVersion) {
      diff += `[+] ${newConfig.nodeVersion}\n`;
    }
  }

  return { diffString: diff };
}

export function setEnvVarsInstructions(
  names: string[],
  deploymentName: string | null,
  deploymentType: DeploymentType | undefined,
  deploymentUrl: string | undefined,
): string {
  // Name cloud deployments explicitly: without a flag `env set` targets the
  // project's default dev deployment, which may not be where this push went
  // (e.g. `npx convex dev --deployment <other dev deployment>`). Local and
  // anonymous deployments are always the default target.
  let deploymentFlag = "";
  if (
    deploymentName !== null &&
    deploymentType !== "local" &&
    deploymentType !== "anonymous"
  ) {
    deploymentFlag = ` --deployment ${deploymentName}`;
  } else if (deploymentType === "prod") {
    deploymentFlag = " --prod";
  } else if (deploymentName === null && deploymentUrl !== undefined) {
    // Pushes via `--url`/`--admin-key` or self-hosted env vars have no
    // deployment name. The admin key stays a placeholder to keep it out of
    // terminal output.
    deploymentFlag = ` --url ${deploymentUrl} --admin-key <admin key>`;
  }
  const commands = (names.length > 0 ? names : ["<NAME>"]).map(
    (name) =>
      `    ${chalkStderr.bold(`npx convex env set${deploymentFlag} ${name} <value>`)}`,
  );
  let instructions = `Set ${names.length > 1 ? "them" : "it"} with:\n\n${commands.join("\n")}\n`;
  // Anonymous deployments are only reachable from the local dashboard, which
  // `deploymentDashboardUrlPage` can't address.
  if (deploymentName !== null && !isAnonymousDeployment(deploymentName)) {
    const variableQuery = names.length === 1 ? `?var=${names[0]}` : "";
    const dashboardUrl = deploymentDashboardUrlPage(
      deploymentName,
      `/settings/environment-variables${variableQuery}`,
    );
    instructions += `\nor in the dashboard:\n\n    ${chalkStderr.bold(dashboardUrl)}\n`;
  }
  return instructions;
}

/** Handle an error from
 * legacy push path:
 * - /api/push_config
 * modern push paths:
 * - /api/deploy2/evaluate_push
 * - /api/deploy2/start_push
 * - /api/deploy2/finish_push
 *
 * finish_push errors are different from start_push errors and in theory could
 * be handled differently, but starting over works for all of them.
 */
export async function handlePushConfigError(
  ctx: Context,
  error: unknown,
  defaultMessage: string,
  deploymentName: string | null,
  deployment:
    | {
        deploymentUrl: string;
        adminKey: string;
        deploymentNotice: string;
      }
    | undefined,
  deploymentType: DeploymentType | undefined,
): Promise<never> {
  const data: ErrorData | undefined =
    error instanceof ThrowingFetchError ? error.serverErrorData : undefined;
  if (data?.code === "AuthConfigMissingEnvironmentVariable") {
    const errorMessage = data.message || "(no error message given)";
    const [, variableName] =
      errorMessage.match(/Environment variable (\S+)/i) ?? [];

    // DEPRECATED: This error path provisioning is being phased out in favor of
    // pre-flight provisioning that happens before the client bundle build.
    // We keep minimal logic here for backwards compatibility with older templates
    // that may still rely on this path.
    if (variableName === "WORKOS_CLIENT_ID" && deploymentName && deployment) {
      // For backwards compatibility with templates that haven't been updated,
      // we'll still show a helpful error message directing users to configure WorkOS.
      // But we no longer do automatic provisioning here since it happens too late
      // (after the client bundle has already been built with missing env vars).
      logWarning(
        "WORKOS_CLIENT_ID is not set; you can set it manually on the deployment or for hosted Convex deployments, use auto-provisioning.",
      );
      logMessage(
        "Learn more at https://docs.convex.dev/auth/authkit/auto-provision",
      );
      logMessage("");
    }

    const envVarMessage =
      `Environment variable ${chalkStderr.bold(
        variableName,
      )} is used in auth config file but ` + `its value was not set.`;
    await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem or env vars",
      errForSentry: error,
      printedMessage:
        envVarMessage +
        "\n" +
        setEnvVarsInstructions(
          variableName !== undefined ? [variableName] : [],
          deploymentName,
          deploymentType,
          deployment?.deploymentUrl,
        ),
    });
  }

  if (data?.code === "MissingEnvironmentVariables") {
    const errorMessage = data.message || "(no error message given)";
    const [, namesList] = errorMessage.match(/are not set: ([^.]+)\./) ?? [];
    const names = namesList?.split(", ") ?? [];
    const envVarMessage =
      names.length === 0
        ? errorMessage
        : names.length === 1
          ? `Environment variable ${chalkStderr.bold(names[0])} is ` +
            `declared as required in convex.config.ts but its value was not set.`
          : `Environment variables ${names
              .map((name) => chalkStderr.bold(name))
              .join(", ")} are declared as required in convex.config.ts ` +
            `but their values were not set.`;
    await ctx.crash({
      exitCode: 1,
      errorType: "invalid filesystem or env vars",
      errForSentry: error,
      printedMessage:
        envVarMessage +
        "\n" +
        setEnvVarsInstructions(
          names,
          deploymentName,
          deploymentType,
          deployment?.deploymentUrl,
        ),
    });
  }

  if (data?.code === "RaceDetected") {
    // Environment variables or schema changed during push. This is a transient
    // error that should be retried immediately with exponential backoff.
    const message =
      data.message || "Schema or environment variables changed during push";
    return await ctx.crash({
      exitCode: 1,
      errorType: "transient",
      errForSentry: error,
      printedMessage: chalkStderr.yellow(message),
    });
  }

  if (data?.code === "InternalServerError") {
    if (deploymentName?.startsWith("local-")) {
      printLocalDeploymentOnError();
      return ctx.crash({
        exitCode: 1,
        errorType: "fatal",
        errForSentry: new LocalDeploymentError(
          "InternalServerError while pushing to local deployment",
        ),
        printedMessage: defaultMessage,
      });
    }
  }

  logFailure(defaultMessage);
  return await logAndHandleFetchError(ctx, error);
}

export type AiFilesProjectConfig = NonNullable<ProjectConfig["aiFiles"]>;

function tryParseJson(raw: string): unknown {
  try {
    return JSON.parse(raw);
  } catch {
    return {};
  }
}

function toRecord(val: unknown): Record<string, unknown> {
  if (val !== null && typeof val === "object" && !Array.isArray(val))
    return val as Record<string, unknown>;
  return {};
}

/**
 * Write the `aiFiles` section of `convex.json`, preserving all other keys.
 * Writes the given object as-is with no merging or default logic.
 * If `aiFiles` is undefined, the key is removed from the file.
 */
export async function writeAiFilesConfig({
  projectDir,
  aiFiles,
}: {
  projectDir: string;
  aiFiles: AiFilesProjectConfig | undefined;
}): Promise<void> {
  const filePath = path.join(projectDir, "convex.json");
  const raw = await nodeFs.readFile(filePath, "utf8").catch(() => null);
  const base = toRecord(raw !== null ? tryParseJson(raw) : {});

  const { $schema, aiFiles: _existing, ...rest } = base;

  const hasContent = aiFiles !== undefined && Object.keys(aiFiles).length > 0;

  const next: Record<string, unknown> = {
    $schema: $schema ?? "node_modules/convex/schemas/convex.schema.json",
    ...rest,
    ...(hasContent ? { aiFiles } : {}),
  };

  await nodeFs.writeFile(
    filePath,
    JSON.stringify(next, null, 2) + "\n",
    "utf8",
  );
}
