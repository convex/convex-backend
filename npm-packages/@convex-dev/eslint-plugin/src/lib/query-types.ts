import type ts from "typescript";

// `OrderedQuery` is an interface, so `getTypeOfSymbolAtLocation` returns an
// error type that every receiver is assignable to, and its declared type is the
// uninstantiated generic, which `OrderedQuery<SomeTable>` is not assignable to.
// Match the interface itself instead: the receiver's type, or the generic
// behind it, is `OrderedQuery` or extends it (as `Query` does).
export function isOrderedQuery(
  type: ts.Type,
  orderedQuerySymbol: ts.Symbol,
  checker: ts.TypeChecker,
): boolean {
  const seen = new Set<ts.Type>();
  const visit = (t: ts.Type): boolean => {
    if (seen.has(t)) return false;
    seen.add(t);
    // `q?.filter(...)` sees `OrderedQuery<...> | undefined`.
    if (t.isUnionOrIntersection()) return t.types.some(visit);
    const target = (t as ts.TypeReference).target ?? t;
    if (target.getSymbol() === orderedQuerySymbol) return true;
    if (!target.isClassOrInterface()) {
      // A type parameter such as `T extends OrderedQuery<...>` is judged by
      // its constraint.
      const constraint = checker.getBaseConstraintOfType(target);
      return constraint ? visit(constraint) : false;
    }
    return (checker.getBaseTypes(target) ?? []).some(visit);
  };
  return visit(type);
}

export function findOrderedQuerySymbol(
  program: ts.Program,
  checker: ts.TypeChecker,
): ts.Symbol | null {
  try {
    for (const sf of program.getSourceFiles()) {
      // Prefer the source file from the convex package.
      // In this monorepo it's `npm-packages/convex/src/server/query.ts`, but be
      // permissive about path separators and .d.ts builds.
      const fname = sf.fileName.replace(/\\/g, "/");
      if (!fname.includes("/convex/")) continue;
      if (
        !fname.endsWith("/server/query.ts") &&
        !fname.endsWith("/server/query.d.ts")
      ) {
        continue;
      }
      const sourceFileSymbol = checker.getSymbolAtLocation(sf);
      if (!sourceFileSymbol) continue;
      const exports = checker.getExportsOfModule(sourceFileSymbol);
      const orderedQuerySymbol = exports.find((e) => e.name === "OrderedQuery");
      if (!orderedQuerySymbol) continue;
      return orderedQuerySymbol;
    }
  } catch {
    // ignore and fall back
  }
  return null;
}
