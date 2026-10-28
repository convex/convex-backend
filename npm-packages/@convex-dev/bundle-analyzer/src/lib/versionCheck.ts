import fs from "fs";
import path from "path";
import { AnalyzerError } from "./output.js";

/**
 * The version of the project's `convex` package: the nearest
 * `node_modules/convex` at or above `projectDir`, which is where Node resolves
 * it from, or null if there isn't one.
 */
export function findProjectConvexVersion(projectDir: string): string | null {
  let dir = path.resolve(projectDir);
  while (true) {
    const file = path.join(dir, "node_modules", "convex", "package.json");
    if (fs.existsSync(file)) {
      let version: unknown;
      try {
        version = JSON.parse(fs.readFileSync(file, "utf8")).version;
      } catch {
        version = undefined;
      }
      if (typeof version !== "string") {
        throw new AnalyzerError(`Couldn't read the version of ${file}.`);
      }
      return version;
    }
    const parent = path.dirname(dir);
    if (parent === dir) return null;
    dir = parent;
  }
}

/**
 * A warning when the project's `convex` version differs from the one this
 * analyzer bundles with. Convex releases don't follow semver strictly, so even
 * a patch release can change bundler behavior.
 */
export function versionMismatchWarning(
  projectConvexVersion: string,
  analyzerConvexVersion: string,
): string | null {
  if (projectConvexVersion === analyzerConvexVersion) {
    return null;
  }
  return `Warning: this project uses convex ${projectConvexVersion}, but this analyzer uses the convex ${analyzerConvexVersion} bundler. Sizes may differ from what \`npx convex deploy\` uploads.`;
}
