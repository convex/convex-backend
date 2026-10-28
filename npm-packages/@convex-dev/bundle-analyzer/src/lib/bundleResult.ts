import type { Metafile } from "esbuild";
import type { BuildDebugInfo } from "convex/bundle-analyzer-internal";

/**
 * The analyzer's only input: what one bundling run produced, as plain
 * JSON-serializable data. Everything except `src/bundle.ts` works from
 * this, so the bundles can come from somewhere other than the convex internals
 * embedded in this package (e.g. a file written by the project's own CLI).
 */
export type BundleResult = {
  /** Absolute path metafile input paths are relative to. */
  projectRoot: string;
  includeSourcesContent: boolean;
  builds: Build[];
};

export type BuildKind = BuildDebugInfo["kind"];

/** One esbuild invocation: one component's build for one runtime. */
export type Build = {
  component: {
    /** Path of the component definition relative to the root, "" for the root app. */
    definitionPath: string;
    /** `app`, an npm package name, or a directory path. */
    label: string;
    isRoot: boolean;
    /** The component's directory, absolute or relative to `projectRoot`. */
    directory: string;
  };
  kind: BuildKind;
  metafile: Metafile;
  /** Modules as uploaded, keyed by `path` (e.g. `messages.js`, `_deps/ABC123.js`). */
  modules: BuildModule[];
  /** Packages the node build leaves external, by name, with their installed versions. */
  externalDependencies: Record<string, string>;
};

export type BuildModule = {
  path: string;
  source: string;
  sourceMap?: string | undefined;
};
