import path from "path";
import type { TSESTree } from "@typescript-eslint/types";
import { ESLintUtils } from "@typescript-eslint/utils";

// List of Convex function registrars to check for
export const CONVEX_REGISTRARS = [
  "query",
  "mutation",
  "action",
  "internalQuery",
  "internalMutation",
  "internalAction",
];

// Registrars that make a function callable by anyone on the internet.
export const PUBLIC_CONVEX_REGISTRARS = ["query", "mutation", "action"];

/**
 * Helper function to get the handler property from an object expression
 */
export function getHandlerProperty(
  objectExpr: TSESTree.ObjectExpression,
): TSESTree.ArrowFunctionExpression | TSESTree.FunctionExpression | null {
  const maybeHandler = objectExpr.properties.find(
    (prop) =>
      prop.type === "Property" &&
      prop.key.type === "Identifier" &&
      prop.key.name === "handler",
  ) as TSESTree.Property | undefined;
  if (!maybeHandler) return null;

  const value = unwrapTSExpression(maybeHandler.value);
  if (
    value.type === "ArrowFunctionExpression" ||
    value.type === "FunctionExpression"
  ) {
    return value;
  }

  return null;
}

/**
 * Unwrap TypeScript-only expression wrappers (`as`, `satisfies`, `!`,
 * `<Type>expr`, `expr<Type>`) that don’t change runtime behavior.
 */
export function unwrapTSExpression(node: TSESTree.Node): TSESTree.Node {
  let result = node;
  while (
    result.type === "TSAsExpression" ||
    result.type === "TSSatisfiesExpression" ||
    result.type === "TSNonNullExpression" ||
    result.type === "TSTypeAssertion" ||
    result.type === "TSInstantiationExpression"
  ) {
    result = result.expression;
  }
  return result;
}

export interface RegisteredFunction {
  call: TSESTree.CallExpression;
  callee: TSESTree.Identifier;
  /** The object argument (new syntax), or null with the old function syntax. */
  objectArg: TSESTree.ObjectExpression | null;
  handler:
    | TSESTree.ArrowFunctionExpression
    | TSESTree.FunctionExpression
    | null;
}

/**
 * If this expression registers a Convex function with one of `registrars`
 * (e.g. `mutation({ handler })` or the old `mutation(async (ctx) => {})`
 * syntax), return its parts.
 */
export function getRegisteredFunction(
  node: TSESTree.Node | null | undefined,
  registrars: string[],
): RegisteredFunction | null {
  if (!node) return null;
  const call = unwrapTSExpression(node);
  if (
    call.type !== "CallExpression" ||
    call.callee.type !== "Identifier" ||
    !registrars.includes(call.callee.name) ||
    call.arguments.length !== 1
  ) {
    return null;
  }

  const argument = unwrapTSExpression(call.arguments[0]);
  const objectArg = argument.type === "ObjectExpression" ? argument : null;
  const handler = objectArg
    ? getHandlerProperty(objectArg)
    : argument.type === "ArrowFunctionExpression" ||
        argument.type === "FunctionExpression"
      ? argument
      : null;
  return { call, callee: call.callee, objectArg, handler };
}

const ENTRY_POINT_EXTENSIONS = [
  // ESBuild js loader
  ".js",
  ".mjs",
  ".cjs",
  // ESBuild ts loader
  ".ts",
  ".tsx",
  ".mts",
  ".cts",
  // ESBuild jsx loader
  ".jsx",
  // ESBuild supports css, text, json, and more but these file types are not
  // allowed to define entry points.
];

/**
 * Assuming this is only called on files in the convex directory,
 * check return true if the file looks like an entry point.
 * This logic matches convex/src/bundler/index.ts.
 */
export function isEntryPoint(fpath: string) {
  const parsedPath = path.parse(fpath);
  const base = parsedPath.base;

  if (!ENTRY_POINT_EXTENSIONS.some((ext) => fpath.endsWith(ext))) {
    return false;
  } else if (fpath.includes("_generated" + path.sep)) {
    return false;
  } else if (base.startsWith(".")) {
    return false;
  } else if (base.startsWith("#")) {
    return false;
  } else if (base === "schema.ts" || base === "schema.js") {
    return false;
  } else if ((base.match(/\./g) || []).length > 1) {
    return false;
  } else if (fpath.includes(" ")) {
    return false;
  } else {
    return true;
  }
}

export const createRule = ESLintUtils.RuleCreator(
  (name) => `https://docs.convex.dev/eslint#${name}`,
);
