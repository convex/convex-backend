import type { TSESTree } from "@typescript-eslint/types";
import { createRule } from "../util.js";
import type {
  ReportFixFunction,
  RuleContext,
} from "@typescript-eslint/utils/ts-eslint";
import { AST_NODE_TYPES } from "@typescript-eslint/types";
import type ts from "typescript";
import { findConvexServerExports } from "./convex-types.js";

/**
 * Rule to enforce explicit table names in database calls
 * (db.get, db.replace, db.patch, db.delete)
 */
export const explicitTableIds = createRule({
  name: "explicit-table-ids",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Database operations should include an explicit table name as the first argument.",
    },
    messages: {
      "missing-table-name":
        "Database {{method}} call should include an explicit table name as the first argument. Expected: db.{{method}}({{tableName}}, ...) ",
      "missing-table-name-no-inference":
        "Database {{method}} call should include an explicit table name as the first argument. Expected: db.{{method}}(<tableName>, ...).",
      "missing-table-name-no-type-info":
        "Database {{method}} call should include an explicit table name as the first argument. Expected: db.{{method}}(<tableName>, ...).\n\nNote: type-aware linting is not enabled in your project. We recommend enabling it in your ESLint configuration (https://typescript-eslint.io/getting-started/typed-linting/), which will allow this rule to provide autofixes and reduce false positives in this rule’s detection logic.",
    },
    schema: [],
    fixable: "code",
  },
  defaultOptions: [],
  create: (context) => {
    const filename = context.filename;

    // Generated files don’t use the DB APIs, so we skip them to avoid unnecessary work
    const isGenerated = filename.includes("_generated");
    if (isGenerated) {
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

    const databaseMethods =
      hasTypeInfo && services?.program
        ? findDatabaseMethodDeclarations(services.program, checker)
        : null;

    return {
      CallExpression(node: TSESTree.CallExpression) {
        // Check if it's a property access (db.get, db.replace, etc.)
        if (node.callee.type !== AST_NODE_TYPES.MemberExpression) {
          return;
        }

        const memberExpr = node.callee;
        if (memberExpr.property.type !== AST_NODE_TYPES.Identifier) {
          return;
        }

        const methodName = memberExpr.property.name;
        if (!(DATABASE_METHODS as readonly string[]).includes(methodName)) {
          return;
        }

        // With Convex's types in the program, the called method decides: it
        // must be one Convex declares on its database interfaces. The pure-AST
        // `<obj>.db.<method>(...)` heuristic covers linting without type info,
        // receivers typed `any`, and programs without Convex's types.
        let isDatabaseCall: boolean | null = null;
        if (hasTypeInfo && databaseMethods) {
          const objectType = checker.getTypeAtLocation(
            tsNodeMap.get(memberExpr.object),
          );
          const objectTypeString = checker.typeToString(objectType);
          if (objectTypeString !== "any" && objectTypeString !== "unknown") {
            const method = checker.getSymbolAtLocation(
              tsNodeMap.get(memberExpr.property),
            );
            isDatabaseCall =
              method?.declarations?.some((d) => databaseMethods.has(d)) ??
              false;
          }
        }
        isDatabaseCall ??=
          memberExpr.object.type === AST_NODE_TYPES.MemberExpression &&
          memberExpr.object.property.type === AST_NODE_TYPES.Identifier &&
          memberExpr.object.property.name === "db";

        if (!isDatabaseCall) {
          return;
        }

        // Check the number of arguments to determine if it's unmigrated
        const args = node.arguments;
        const isUnmigrated =
          (methodName === "get" && args.length === 1) ||
          (methodName === "replace" && args.length === 2) ||
          (methodName === "patch" && args.length === 2) ||
          (methodName === "delete" && args.length === 1);

        if (!isUnmigrated) {
          return;
        }

        // Try to get type information for the first argument. Table-name
        // inference (and therefore the autofix) is only possible with type info.
        let tableName: string | null = null;

        if (hasTypeInfo) {
          const tsNode = tsNodeMap.get(args[0]);
          const type = checker.getTypeAtLocation(tsNode);

          // Try to extract table name from Id<"tableName"> type
          if (type.aliasSymbol?.name === "Id") {
            // Type with alias type arguments (internal TypeScript API)
            const typeWithArgs = type as ts.Type & {
              aliasTypeArguments?: readonly ts.Type[];
            };
            const typeArgs = typeWithArgs.aliasTypeArguments;
            if (typeArgs && typeArgs.length === 1) {
              const tableType = typeArgs[0];
              // Check if it's a string literal type
              if (tableType.isStringLiteral && tableType.isStringLiteral()) {
                tableName = tableType.value;
              } else if (tableType.flags & (1 << 7)) {
                // StringLiteral flag = 128 = 1 << 7
                // Fallback for different TypeScript versions
                const stringLiteralType = tableType as ts.StringLiteralType;
                tableName = stringLiteralType.value;
              }
            }
          }
        }

        // Report the issue
        if (tableName) {
          context.report({
            node,
            messageId: "missing-table-name",
            data: {
              method: methodName,
              tableName: JSON.stringify(tableName),
            },
            fix: createTableNameFix(context, node, tableName),
          });
        } else {
          context.report({
            node,
            messageId: hasTypeInfo
              ? "missing-table-name-no-inference"
              : "missing-table-name-no-type-info",
            data: {
              method: methodName,
            },
          });
        }
      },
    };
  },
});

const DATABASE_METHODS = ["get", "replace", "patch", "delete"] as const;

/**
 * Declarations of the database methods that take an optional table name.
 * `GenericDatabaseWriter` inherits `get` from `BaseDatabaseReader`, so these
 * also match `get` on `GenericDatabaseReader` and on `db.system`. Matching the
 * declarations rather than type names leaves alone other libraries' methods,
 * such as table-scoped writers whose `patch(id, value)` already knows its
 * table.
 */
function findDatabaseMethodDeclarations(
  program: ts.Program,
  checker: ts.TypeChecker,
): Set<ts.Declaration> | null {
  const declarations = new Set<ts.Declaration>();
  for (const writer of findConvexServerExports(
    program,
    checker,
    "database",
    "GenericDatabaseWriter",
  )) {
    const writerType = checker.getDeclaredTypeOfSymbol(writer);
    for (const method of DATABASE_METHODS) {
      for (const d of writerType.getProperty(method)?.declarations ?? []) {
        declarations.add(d);
      }
    }
  }
  return declarations.size > 0 ? declarations : null;
}

/**
 * Creates a fix that inserts the table name as the first argument
 */
function createTableNameFix(
  context: RuleContext<string, unknown[]>,
  call: TSESTree.CallExpression,
  tableName: string,
): ReportFixFunction {
  return (fixer) => {
    const firstArg = call.arguments[0];
    if (!firstArg) return null;

    const tableNameString = JSON.stringify(tableName);
    return fixer.insertTextBefore(firstArg, `${tableNameString}, `);
  };
}
