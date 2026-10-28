import type { TSESTree } from "@typescript-eslint/utils";
import { AST_NODE_TYPES } from "@typescript-eslint/utils";

export const TERMINAL_QUERY_METHODS = new Set([
  "collect",
  "take",
  "first",
  "unique",
  "paginate",
]);

export function unwrapExpression(
  expr: TSESTree.Expression,
): TSESTree.Expression {
  let current: TSESTree.Expression = expr;
  while (true) {
    // ignore `?` (e.g. `a?.b`)
    if (current.type === AST_NODE_TYPES.ChainExpression) {
      current = current.expression;
      continue;
    }

    // ignore `await`
    if (current.type === AST_NODE_TYPES.AwaitExpression) {
      current = current.argument;
      continue;
    }

    // ignore `!`
    if (current.type === AST_NODE_TYPES.TSNonNullExpression) {
      current = current.expression;
      continue;
    }

    // ignore `as`
    if (current.type === AST_NODE_TYPES.TSAsExpression) {
      current = current.expression;
      continue;
    }

    // ignore `<Type>` in `<Type>value`
    if (current.type === AST_NODE_TYPES.TSTypeAssertion) {
      current = current.expression;
      continue;
    }

    // Ignore generic arguments (e.g. `<T>` in `fn<T>()`)
    if (current.type === AST_NODE_TYPES.TSInstantiationExpression) {
      current = current.expression;
      continue;
    }

    // Ignore parentheses
    const anyCurrent =
      // `ParenthesizedExpression` isn't present in all @typescript-eslint AST_NODE_TYPES versions.
      current as any;
    if (anyCurrent.type === "ParenthesizedExpression") {
      current = anyCurrent.expression as TSESTree.Expression;
      continue;
    }
    return current;
  }
}

// Whether the node is a call to a function that collects a query
export function isTerminalQueryCall(expr: TSESTree.Expression): boolean {
  const unwrapped = unwrapExpression(expr);
  if (unwrapped.type !== AST_NODE_TYPES.CallExpression) return false;

  const callee = unwrapped.callee;
  if (callee.type !== AST_NODE_TYPES.MemberExpression) return false;
  if (callee.property.type !== AST_NODE_TYPES.Identifier) return false;
  return TERMINAL_QUERY_METHODS.has(callee.property.name);
}

export function isDbQueryChainFallback(expr: TSESTree.Expression): boolean {
  const unwrapped = unwrapExpression(expr);
  if (unwrapped.type !== AST_NODE_TYPES.CallExpression) return false;

  const callee = unwrapped.callee;
  if (callee.type !== AST_NODE_TYPES.MemberExpression) return false;
  if (callee.property.type !== AST_NODE_TYPES.Identifier) return false;

  const methodName = callee.property.name;
  if (TERMINAL_QUERY_METHODS.has(methodName)) {
    // `db.query(...).collect()` and friends return arrays / values, not a query builder.
    return false;
  }

  if (methodName === "query") {
    const baseObject = unwrapExpression(callee.object as TSESTree.Expression);
    // `ctx.db.query(...)`
    if (
      baseObject.type === AST_NODE_TYPES.MemberExpression &&
      baseObject.property.type === AST_NODE_TYPES.Identifier &&
      baseObject.property.name === "db"
    ) {
      return true;
    }
    // `db.query(...)` (DatabaseReader / DatabaseWriter passed around)
    if (
      baseObject.type === AST_NODE_TYPES.Identifier &&
      baseObject.name === "db"
    ) {
      return true;
    }
    // `db.privateSystem.query(...)` (system UDFs)
    if (
      baseObject.type === AST_NODE_TYPES.MemberExpression &&
      baseObject.property.type === AST_NODE_TYPES.Identifier &&
      baseObject.property.name === "privateSystem"
    ) {
      const innerObject = unwrapExpression(
        baseObject.object as TSESTree.Expression,
      );
      if (
        innerObject.type === AST_NODE_TYPES.Identifier &&
        innerObject.name === "db"
      ) {
        return true;
      }
      if (
        innerObject.type === AST_NODE_TYPES.MemberExpression &&
        innerObject.property.type === AST_NODE_TYPES.Identifier &&
        innerObject.property.name === "db"
      ) {
        return true;
      }
    }
    return false;
  }

  // Continue walking down chained calls like `.withIndex(...)`, `.order(...)`, etc.
  return isDbQueryChainFallback(callee.object as TSESTree.Expression);
}
