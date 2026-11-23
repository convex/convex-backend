import type ts from "typescript";
import { findConvexServerExports } from "./convex-types.js";

// `OrderedQuery` is an interface, so `getTypeOfSymbolAtLocation` returns an
// error type that every receiver is assignable to, and its declared type is the
// uninstantiated generic, which `OrderedQuery<SomeTable>` is not assignable to.
// Match the interface itself instead: the receiver's type, or the generic
// behind it, is `OrderedQuery` or extends it (as `Query` does).
export function isOrderedQuery(
  type: ts.Type,
  orderedQuerySymbols: ReadonlySet<ts.Symbol>,
  checker: ts.TypeChecker,
): boolean {
  const seen = new Set<ts.Type>();
  const visit = (t: ts.Type): boolean => {
    if (seen.has(t)) return false;
    seen.add(t);
    // `q?.filter(...)` sees `OrderedQuery<...> | undefined`.
    if (t.isUnionOrIntersection()) return t.types.some(visit);
    const target = (t as ts.TypeReference).target ?? t;
    const symbol = target.getSymbol();
    if (symbol && orderedQuerySymbols.has(symbol)) return true;
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

export function findOrderedQuerySymbols(
  program: ts.Program,
  checker: ts.TypeChecker,
): Set<ts.Symbol> | null {
  const symbols = findConvexServerExports(
    program,
    checker,
    "query",
    "OrderedQuery",
  );
  return symbols.length > 0 ? new Set(symbols) : null;
}
