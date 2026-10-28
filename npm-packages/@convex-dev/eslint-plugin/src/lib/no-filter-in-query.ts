import type { TSESTree } from "@typescript-eslint/utils";
import { AST_NODE_TYPES } from "@typescript-eslint/utils";
import { createRule } from "../util.js";
import type ts from "typescript";
import {
  isDbQueryChainFallback,
  isTerminalQueryCall,
  unwrapExpression,
} from "./query-ast.js";
import { findOrderedQuerySymbol, isOrderedQuery } from "./query-types.js";

type MessageIds = "no-filter-in-query";

export const noFilterInQuery = createRule<[], MessageIds>({
  name: "no-filter-in-query",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Warn when using `.filter()` on a Convex database query, since it can be inefficient.",
    },
    messages: {
      "no-filter-in-query":
        "Avoid calling `.filter()` on a Convex database query; it can be inefficient (see Convex query best practices). If you have a real reason to use it, disable with `// eslint-disable-next-line @convex-dev/no-filter-in-query`.",
    },
    schema: [],
  },
  defaultOptions: [],
  create: (context) => {
    const filename = context.filename;
    if (filename.includes("_generated")) {
      return {};
    }

    const services = context.sourceCode.parserServices;
    const checker = services?.program?.getTypeChecker?.();
    const tsNodeMap = services?.esTreeNodeToTSNodeMap;
    const hasTypeInfo = !!(checker && tsNodeMap);

    const orderedQuerySymbol =
      hasTypeInfo && services?.program
        ? findOrderedQuerySymbol(services.program, checker)
        : null;

    // Variables initialized from a `db.query(...)` chain, so `q.filter(...)`
    // is still recognized when the type checker sees `q` as `any`.
    const queryVarSymbols = new Set<ts.Symbol>();

    function isIdentifierExplicitAnyType(id: TSESTree.Identifier): boolean {
      return (
        id.typeAnnotation?.typeAnnotation.type === AST_NODE_TYPES.TSAnyKeyword
      );
    }

    return {
      CallExpression(node: TSESTree.CallExpression) {
        if (node.callee.type !== AST_NODE_TYPES.MemberExpression) return;

        const memberExpr = node.callee;
        if (memberExpr.property.type !== AST_NODE_TYPES.Identifier) return;

        // Track `const q = db.query(...);` style query builders so we can
        // later recognize `q.filter(...)`.
        if (
          checker &&
          tsNodeMap &&
          memberExpr.property.name === "query" &&
          node.parent?.type === AST_NODE_TYPES.VariableDeclarator &&
          node.parent.id.type === AST_NODE_TYPES.Identifier
        ) {
          const varDecl = node.parent;
          const initExpr = varDecl.init as TSESTree.Expression | null;
          if (initExpr && isDbQueryChainFallback(initExpr)) {
            try {
              const idTsNode = tsNodeMap.get(varDecl.id);
              const sym = checker.getSymbolAtLocation(idTsNode as any);
              if (
                sym &&
                !isIdentifierExplicitAnyType(varDecl.id as TSESTree.Identifier)
              ) {
                queryVarSymbols.add(sym);
              }
            } catch {
              // ignore
            }
          }
        }

        if (memberExpr.property.name !== "filter") return;

        const receiver = memberExpr.object as TSESTree.Expression;

        // Don’t warn for array `.filter()` after running a query, e.g.
        // `(await db.query(...).collect()).filter(...)`.
        if (isTerminalQueryCall(receiver)) return;

        let isConvexQuery = false;
        let shouldUseAstFallback = !hasTypeInfo;
        if (hasTypeInfo) {
          try {
            const receiverUnwrapped = unwrapExpression(receiver);
            const receiverTsNode = tsNodeMap.get(receiverUnwrapped);
            const receiverType = checker.getTypeAtLocation(
              receiverTsNode as any,
            );
            const receiverTypeString = checker.typeToString(receiverType);
            if (
              receiverTypeString === "any" ||
              receiverTypeString === "unknown"
            ) {
              const sym =
                receiverUnwrapped.type === AST_NODE_TYPES.Identifier
                  ? checker.getSymbolAtLocation(receiverTsNode as any)
                  : undefined;
              isConvexQuery = !!sym && queryVarSymbols.has(sym);
              shouldUseAstFallback = true;
            } else {
              // The receiver's type gives a concrete verdict; don't let the
              // AST heuristic override it.
              isConvexQuery =
                !!orderedQuerySymbol &&
                isOrderedQuery(receiverType, orderedQuerySymbol, checker);
            }
          } catch {
            // Fall back below.
            shouldUseAstFallback = true;
          }
        }

        // Fallback when linting is non type-aware or type info is inconclusive.
        if (!isConvexQuery && shouldUseAstFallback) {
          isConvexQuery = isDbQueryChainFallback(receiver);
        }

        if (isConvexQuery) {
          context.report({
            node: memberExpr.property,
            messageId: "no-filter-in-query",
          });
        }
      },
    };
  },
});
