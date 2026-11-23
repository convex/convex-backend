import type ts from "typescript";

/**
 * Finds an export of a module in the `convex` package's `server` directory,
 * e.g. `OrderedQuery` from `server/query`, in every copy of the package in the
 * program: a dependency can pin a different version of `convex` than the app's.
 * Matches both the package's sources (`npm-packages/convex/src/server/query.ts`
 * in this monorepo) and its `.d.ts` builds, with either path separator.
 */
export function findConvexServerExports(
  program: ts.Program,
  checker: ts.TypeChecker,
  module: string,
  name: string,
): ts.Symbol[] {
  const symbols: ts.Symbol[] = [];
  try {
    for (const sf of program.getSourceFiles()) {
      const fname = sf.fileName.replace(/\\/g, "/");
      if (!fname.includes("/convex/")) continue;
      if (
        !fname.endsWith(`/server/${module}.ts`) &&
        !fname.endsWith(`/server/${module}.d.ts`)
      ) {
        continue;
      }
      const sourceFileSymbol = checker.getSymbolAtLocation(sf);
      if (!sourceFileSymbol) continue;
      const symbol = checker
        .getExportsOfModule(sourceFileSymbol)
        .find((e) => e.name === name);
      if (symbol) symbols.push(symbol);
    }
  } catch {
    // ignore and fall back
  }
  return symbols;
}
