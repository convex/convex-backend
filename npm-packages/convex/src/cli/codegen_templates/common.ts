export function header(oneLineDescription: string) {
  return `/* eslint-disable */
  /**
   * ${oneLineDescription}
   *
   * THIS CODE IS AUTOMATICALLY GENERATED.
   *
   * To regenerate, run \`npx convex dev\`.
   * @module
   */
  `;
}

export function apiComment(
  apiName: string,
  type: "public" | "internal" | undefined,
) {
  return `
  /**
     * A utility for referencing Convex functions in your app's${type ? ` ${type}` : ""} API.
     *
     * Usage:
     * \`\`\`js
     * const myFunctionReference = ${apiName}.myModule.myFunction;
     * \`\`\`
     */`;
}

const collator = new Intl.Collator("en-US", {
  usage: "sort",
  numeric: true,
  sensitivity: "case",
  ignorePunctuation: false,
  caseFirst: "false",
});

/**
 * Comparison function for sorting strings alphabetically.
 *
 * Usage: array.sort(compareStrings)
 * or with entries: Object.entries(obj).sort(([a], [b]) => compareStrings(a, b))
 */
export function compareStrings(a: string, b: string): number {
  return collator.compare(a, b);
}

/**
 * Comparison function for sorting module paths in codegen output.
 *
 * Compares the forward slash normalized form of each path so the resulting
 * order is identical on every platform. Sorting OS-native paths directly
 * diverges on Windows because "\" (0x5C) sorts after letters while "/"
 * (0x2F) sorts before them. Uses plain code unit comparison to match the
 * default Array.prototype.sort() order these paths sorted with on POSIX.
 *
 * Usage: modulePaths.sort(compareModulePaths)
 */
export function compareModulePaths(a: string, b: string): number {
  const aNormalized = a.replace(/\\/g, "/");
  const bNormalized = b.replace(/\\/g, "/");
  return aNormalized < bNormalized ? -1 : aNormalized > bNormalized ? 1 : 0;
}
