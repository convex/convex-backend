import type { TSESTree } from "@typescript-eslint/types";
import { AST_NODE_TYPES } from "@typescript-eslint/types";
import { createRule } from "../util.js";
import type ts from "typescript";
import { isDbQueryChainFallback } from "./query-ast.js";

type Options = [];
type MessageIds =
  | "no-collect-in-query"
  | "no-collect-in-query-no-type-info"
  | "replace-with-paginate"
  | "replace-with-take";

function isCollectCall(
  node: TSESTree.CallExpression,
): node is TSESTree.CallExpression & {
  callee: TSESTree.MemberExpression & {
    computed: false;
    property: TSESTree.Identifier & { name: "collect" };
  };
} {
  if (node.callee.type !== AST_NODE_TYPES.MemberExpression) return false;
  if (node.callee.computed) return false;
  if (node.callee.property.type !== AST_NODE_TYPES.Identifier) return false;
  return node.callee.property.name === "collect";
}

/**
 * Find the declaration of the `collect()` method that Convex's `OrderedQuery`
 * interface declares.
 *
 * Convex query types are recognized by the *identity* of that declaration
 * rather than by assignability to the `OrderedQuery` type. `OrderedQuery` is a
 * generic interface, so an assignability check against its uninstantiated
 * declaration is both unsound (`getTypeOfSymbolAtLocation` on a type-only
 * symbol returns the error type `any`, which accepts every receiver) and
 * incomplete (`OrderedQuery<T>` is invariant in `T`, so a concrete
 * `OrderedQuery<MyTable>` is not assignable to the bare declaration).
 */
function findOrderedQueryCollectDeclaration(
  program: ts.Program,
  checker: ts.TypeChecker,
): ts.Declaration | null {
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
      // `getTypeOfSymbolAtLocation` would return the error type `any` here
      // because `OrderedQuery` is an interface and has no value meaning.
      const collectDeclaration = checker
        .getDeclaredTypeOfSymbol(orderedQuerySymbol)
        .getProperty("collect")?.declarations?.[0];
      if (!collectDeclaration) continue;
      return collectDeclaration;
    }
  } catch {
    // ignore and fall back
  }
  return null;
}

/**
 * Whether `.collect()` called on `type` is the `collect()` declared by Convex's
 * `OrderedQuery`, i.e. whether `type` is a Convex query.
 */
function isConvexQueryType(
  type: ts.Type,
  orderedQueryCollect: ts.Declaration,
): boolean {
  // A union receiver only qualifies when every constituent is a Convex query,
  // mirroring how assignability would have treated it.
  if (type.isUnion()) {
    return type.types.every((t) => isConvexQueryType(t, orderedQueryCollect));
  }
  return type.getProperty("collect")?.declarations?.[0] === orderedQueryCollect;
}

/**
 * Rule to discourage calling `.collect()` on Convex queries.
 */
export const noCollectInQuery = createRule<Options, MessageIds>({
  name: "no-collect-in-query",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Disallow calling `.collect()` on Convex queries; prefer `.take()` or `.paginate()` instead.",
    },
    schema: [],
    hasSuggestions: true,
    messages: {
      "no-collect-in-query":
        "Avoid calling `.collect()` in a Convex query: it can fail for large datasets. Prefer `.take()` or `.paginate()` instead (see the best practices docs). If you are certain that this call to `.collect()` won’t reach the [Convex query limits](https://docs.convex.dev/production/state/limits), you can disable this line with `// eslint-disable-next-line @convex-dev/no-collect-in-query`.",
      "no-collect-in-query-no-type-info":
        "Avoid calling `.collect()` in a Convex query: it can fail for large datasets. Prefer `.take()` or `.paginate()` instead (see the best practices docs). If you are certain that this call to `.collect()` won’t reach the [Convex query limits](https://docs.convex.dev/production/state/limits), you can disable this line with `// eslint-disable-next-line @convex-dev/no-collect-in-query`.\n\nNote: type-aware linting is not enabled in your project, so the detection logic used by this linting rule is less precise. If this is a false positive, consider enabling type-aware linting in your ESLint configuration (https://typescript-eslint.io/getting-started/typed-linting/).",
      "replace-with-take": "Replace `.collect()` with `.take()`.",
      "replace-with-paginate": "Replace `.collect()` with `.paginate()`.",
    },
  },
  defaultOptions: [],
  create: (context) => {
    const filename = context.filename;

    // Generated files don’t use the DB APIs, so we skip them to avoid unnecessary work
    if (filename.includes("_generated")) {
      return {};
    }

    const services = context.sourceCode.parserServices;
    const checker = services?.program?.getTypeChecker?.();
    const tsNodeMap = services?.esTreeNodeToTSNodeMap;
    const hasTypeInfo = !!(
      checker &&
      tsNodeMap &&
      typeof tsNodeMap.get === "function"
    );

    // Resolve Convex's `OrderedQuery.collect()` declaration once (only possible
    // when type info is available).
    const orderedQueryCollectDeclaration =
      hasTypeInfo && services?.program
        ? findOrderedQueryCollectDeclaration(services.program, checker)
        : null;

    return {
      CallExpression(node) {
        if (!isCollectCall(node)) return;

        // Type-aware path: trust the type checker. Recognize the receiver as a
        // Convex query and offer autofix suggestions. When type info is
        // available we intentionally do NOT fall back to the AST heuristic:
        // the heuristic matches `db.query(...)` chains by name and would
        // produce false positives on non-Convex types the checker can see are
        // unrelated.
        if (hasTypeInfo) {
          // If we couldn't resolve `OrderedQuery`, skip to avoid false
          // positives (the file likely doesn't use Convex at all).
          if (!orderedQueryCollectDeclaration) {
            return;
          }

          // Avoid warning on `any` or other unresolved types.
          const objectTsNode = tsNodeMap.get(node.callee.object);
          const objectType = checker.getTypeAtLocation(objectTsNode);
          if (checker.typeToString(objectType) === "any") {
            return;
          }

          if (!isConvexQueryType(objectType, orderedQueryCollectDeclaration)) {
            return;
          }

          context.report({
            node,
            messageId: "no-collect-in-query",
            suggest: [
              {
                messageId: "replace-with-take",
                fix: (fixer) => fixer.replaceText(node.callee.property, "take"),
              },
              {
                messageId: "replace-with-paginate",
                fix: (fixer) =>
                  fixer.replaceText(node.callee.property, "paginate"),
              },
            ],
          });
          return;
        }

        // Fallback path (no type info): use a pure-AST heuristic to detect
        // `db.query(...)....collect()` chains. Autofixes/suggestions need type
        // info to be safe, so we omit them here.
        if (isDbQueryChainFallback(node.callee.object as TSESTree.Expression)) {
          context.report({
            node,
            messageId: "no-collect-in-query-no-type-info",
          });
        }
      },
    };
  },
});
