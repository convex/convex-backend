import type { Metafile } from "esbuild";
import type { BuildKind } from "./bundleResult.js";

export type { BuildKind };

/**
 * Where a bundled input file came from:
 * - `app`: the component's own source (the root app's `convex/` directory, or
 *   a component package's own code)
 * - `generated`: `_generated/` files written by codegen for this component or
 *   the project. Another package's `_generated/` files, such as a component
 *   package's client code imported by the app, count toward that package.
 * - `convex`: the `convex` npm package
 * - `npm`: any other package
 * - `virtual`: files synthesized by bundler plugins (shims, stubs, wasm)
 */
export type SourceOrigin = "app" | "generated" | "convex" | "npm" | "virtual";

export type InputAttribution = {
  origin: SourceOrigin;
  packageName: string | null;
  packageVersion: string | null;
  /** Directory of the package's package.json, relative to the project root. */
  packageDir: string | null;
};

export type ComponentInfo = {
  /** Path of the component definition relative to the root, "" for the root app. */
  definitionPath: string;
  label: string;
  isRoot: boolean;
};

/** A package the node build leaves external, installed on the server instead. */
export type ExternalPackageInfo = {
  name: string;
  version: string;
  /** Unzipped size on disk of the locally installed package directory. */
  diskBytes: number | null;
};

export type ModuleSizes = {
  codeBytes: number;
  sourceMapBytes: number;
  /** Source map bytes per metafile input path. */
  sourceMapBytesByInput: Map<string, number>;
  /**
   * Source map bytes that belong to no source: `names`, JSON structure, and
   * mappings without a source.
   */
  sourceMapOverheadBytes: number;
  /**
   * Source map bytes for sources that don't resolve to any input, keyed by the
   * resolved source. With `sourceMapBytesByInput` and
   * `sourceMapOverheadBytes`, sums to `sourceMapBytes`.
   */
  sourceMapUnmatchedBytesBySource: Map<string, number>;
};

/** One esbuild invocation's results, measured. */
export type BuildRecord = {
  component: ComponentInfo;
  kind: BuildKind;
  metafile: Metafile;
  /** Keyed by module path as uploaded (e.g. `messages.js`, `_deps/ABC123.js`). */
  modules: Map<string, ModuleSizes>;
  /** Keyed by metafile input path. */
  attribution: Map<string, InputAttribution>;
  externalPackages: ExternalPackageInfo[];
};

export type CollectedBundle = {
  /** Absolute path metafile input paths are relative to. */
  projectRoot: string;
  convexVersion: string;
  includeSourcesContent: boolean;
  builds: BuildRecord[];
};

export type AnalyzedInput = {
  path: string;
  bytesInOutput: number;
  sourceMapBytes: number;
} & InputAttribution;

export type AnalyzedOutput = {
  /** Module path as uploaded. */
  path: string;
  /**
   * Metafile input path of the entry point. Null for shared chunks and for
   * chunks esbuild splits out for dynamic `import()` targets.
   */
  entryPoint: string | null;
  codeBytes: number;
  sourceMapBytes: number;
  /** Module paths of chunks this output statically imports. */
  imports: string[];
  /** Module paths of chunks this output loads with dynamic `import()`. */
  dynamicImports: string[];
  inputs: AnalyzedInput[];
  /** Code esbuild emitted that isn't attributed to any input (runtime helpers, glue). */
  unattributedBytes: number;
  /** See `ModuleSizes.sourceMapOverheadBytes`. */
  sourceMapOverheadBytes: number;
  /** See `ModuleSizes.sourceMapUnmatchedBytesBySource`. */
  sourceMapUnmatchedBytesBySource: Map<string, number>;
};

export type AnalyzedBuild = BuildRecord & {
  id: string;
  outputs: AnalyzedOutput[];
};

/** Installed packages count once per name and version across all builds. */
export function distinctExternalPackages(
  builds: AnalyzedBuild[],
): ExternalPackageInfo[] {
  const packages = new Map<string, ExternalPackageInfo>();
  for (const b of builds) {
    for (const pkg of b.externalPackages) {
      const key = `${pkg.name}@${pkg.version}`;
      const previous = packages.get(key);
      if (
        !previous ||
        (previous.diskBytes === null && pkg.diskBytes !== null)
      ) {
        packages.set(key, pkg);
      }
    }
  }
  return [...packages.values()];
}

export type Analysis = {
  bundle: CollectedBundle;
  builds: AnalyzedBuild[];
};

export function buildId(b: Pick<BuildRecord, "component" | "kind">): string {
  return `${b.component.label}:${b.kind}`;
}

const OUT_PREFIX = "out/";

export function modulePathFromMetafileOutput(outputKey: string): string {
  const posix = outputKey.split("\\").join("/");
  return posix.startsWith(OUT_PREFIX) ? posix.slice(OUT_PREFIX.length) : posix;
}

/**
 * Bytes an input adds to the upload: its code in the output plus its share of
 * the source map. Both are uploaded and count toward the deployment size limits.
 */
export function inputBytes(
  input: Pick<AnalyzedInput, "bytesInOutput" | "sourceMapBytes">,
): number {
  return input.bytesInOutput + input.sourceMapBytes;
}

export function outputBytes(
  output: Pick<AnalyzedOutput, "codeBytes" | "sourceMapBytes">,
): number {
  return output.codeBytes + output.sourceMapBytes;
}

export const CODE_GLUE_NAME = "(esbuild glue)";
export const MAP_OVERHEAD_NAME = "(source map overhead)";
export const UNMATCHED_SOURCES_NAME = "(unmatched sources)";

/** Source map bytes for sources that belong to a file the analyzer couldn't match to an input. */
export function unmatchedSourceMapBytes(
  output: Pick<AnalyzedOutput, "sourceMapUnmatchedBytesBySource">,
): number {
  let total = 0;
  for (const bytes of output.sourceMapUnmatchedBytesBySource.values()) {
    total += bytes;
  }
  return total;
}

/**
 * Bytes of an output not attributed to any input. With the inputs'
 * `inputBytes`, sums to `outputBytes`.
 */
export function unattributedBytes(
  output: Pick<
    AnalyzedOutput,
    | "unattributedBytes"
    | "sourceMapOverheadBytes"
    | "sourceMapUnmatchedBytesBySource"
  >,
): number {
  return (
    output.unattributedBytes +
    output.sourceMapOverheadBytes +
    unmatchedSourceMapBytes(output)
  );
}

export function analyze(bundle: CollectedBundle): Analysis {
  const builds = bundle.builds.map((record): AnalyzedBuild => {
    const outputs: AnalyzedOutput[] = [];
    for (const [key, output] of Object.entries(record.metafile.outputs)) {
      if (key.endsWith(".map")) continue;
      const modulePath = modulePathFromMetafileOutput(key);
      const sizes = record.modules.get(modulePath) ?? {
        codeBytes: output.bytes,
        sourceMapBytes: 0,
        sourceMapBytesByInput: new Map(),
        sourceMapOverheadBytes: 0,
        sourceMapUnmatchedBytesBySource: new Map(),
      };
      const inputPaths = new Set([
        ...Object.keys(output.inputs),
        ...sizes.sourceMapBytesByInput.keys(),
      ]);
      const inputs: AnalyzedInput[] = [...inputPaths].map((inputPath) => ({
        path: inputPath,
        // esbuild's metafile is a plain object keyed by path.
        bytesInOutput: Object.hasOwn(output.inputs, inputPath)
          ? output.inputs[inputPath].bytesInOutput
          : 0,
        sourceMapBytes: sizes.sourceMapBytesByInput.get(inputPath) ?? 0,
        ...(record.attribution.get(inputPath) ?? unknownAttribution()),
      }));
      inputs.sort((a, b) => b.bytesInOutput - a.bytesInOutput);
      const attributedCode = inputs.reduce((n, i) => n + i.bytesInOutput, 0);
      outputs.push({
        path: modulePath,
        entryPoint:
          output.entryPoint && !modulePath.startsWith("_deps/")
            ? output.entryPoint
            : null,
        codeBytes: sizes.codeBytes,
        sourceMapBytes: sizes.sourceMapBytes,
        imports: output.imports
          .filter((i) => i.kind === "import-statement" && !i.external)
          .map((i) => modulePathFromMetafileOutput(i.path)),
        dynamicImports: output.imports
          .filter((i) => i.kind === "dynamic-import" && !i.external)
          .map((i) => modulePathFromMetafileOutput(i.path)),
        inputs,
        unattributedBytes: Math.max(0, sizes.codeBytes - attributedCode),
        sourceMapOverheadBytes: sizes.sourceMapOverheadBytes,
        sourceMapUnmatchedBytesBySource: sizes.sourceMapUnmatchedBytesBySource,
      });
    }
    outputs.sort((a, b) => b.codeBytes - a.codeBytes);
    return { ...record, id: buildId(record), outputs };
  });
  return { bundle, builds };
}

function unknownAttribution(): InputAttribution {
  return {
    origin: "virtual",
    packageName: null,
    packageVersion: null,
    packageDir: null,
  };
}

/** Label used to group inputs: a package (`name@version`) or an origin bucket. */
export function ownerLabel(input: InputAttribution): string {
  switch (input.origin) {
    case "app":
      return "your code";
    case "generated":
      return "_generated";
    case "virtual":
      return "(bundler shims)";
    case "convex":
    case "npm":
      return input.packageVersion
        ? `${input.packageName}@${input.packageVersion}`
        : (input.packageName ?? "(unknown package)");
  }
}

/** A file path shortened to be relative to its package, for display. */
export function displayInputPath(input: AnalyzedInput): string {
  if (input.packageDir && input.path.startsWith(input.packageDir + "/")) {
    return input.path.slice(input.packageDir.length + 1);
  }
  return input.path.replace(/^(\.\.\/)+/, "");
}

/**
 * A file path for display with its package, e.g. `zod@3.25.76/index.js`, or
 * the path itself for files outside packages.
 */
export function displayPath(
  inputPath: string,
  attribution: InputAttribution | undefined,
): string {
  if (attribution?.packageName && attribution.packageDir) {
    const rel = inputPath.startsWith(attribution.packageDir + "/")
      ? inputPath.slice(attribution.packageDir.length + 1)
      : inputPath;
    return `${ownerLabel(attribution)}/${rel}`;
  }
  return inputPath.replace(/^(\.\.\/)+/, "");
}
