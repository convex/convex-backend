import type { TSESTree } from "@typescript-eslint/types";
import { createRule } from "../util.js";
import type {
  ReportFixFunction,
  RuleContext,
} from "@typescript-eslint/utils/ts-eslint";
import { AST_NODE_TYPES } from "@typescript-eslint/types";
import type ts from "typescript";

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

    // Get DatabaseReader and DatabaseWriter types for proper subtype checking
    // We need to find these types in the type system
    let anyDatabaseReader: ts.Type | null = null;
    let anyDatabaseWriter: ts.Type | null = null;

    if (hasTypeInfo && services?.program) {
      try {
        // Try to get the database types from the program
        const sourceFiles = services.program.getSourceFiles();
        for (const sf of sourceFiles) {
          if (sf.fileName.includes("_generated/server")) {
            const sourceFileSymbol = checker.getSymbolAtLocation(sf);
            if (sourceFileSymbol) {
              const exports = checker.getExportsOfModule(sourceFileSymbol);
              for (const exp of exports) {
                const type = checker.getTypeOfSymbolAtLocation(exp, sf);
                const typeString = checker.typeToString(type);
                if (
                  typeString.includes("DatabaseReader") ||
                  typeString.includes("GenericDatabaseReader")
                ) {
                  // Get the type that has the methods we care about
                  const dbProp = type.getProperty("db");
                  if (dbProp) {
                    const dbType = checker.getTypeOfSymbolAtLocation(
                      dbProp,
                      sf,
                    );
                    anyDatabaseReader = dbType;
                  }
                }
                if (
                  typeString.includes("DatabaseWriter") ||
                  typeString.includes("GenericDatabaseWriter")
                ) {
                  const dbProp = type.getProperty("db");
                  if (dbProp) {
                    const dbType = checker.getTypeOfSymbolAtLocation(
                      dbProp,
                      sf,
                    );
                    anyDatabaseWriter = dbType;
                  }
                }
              }
            }
            break;
          }
        }
      } catch {
        // If we can't get the types, we'll fall back to pattern matching
      }
    }

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
        const validMethods = ["get", "replace", "patch", "delete"];
        if (!validMethods.includes(methodName)) {
          return;
        }

        // Check if the object is a database by checking its type or pattern.
        // The type-based check only runs when type info is available.
        let isDatabaseType = false;
        if (hasTypeInfo) {
          const objectTsNode = tsNodeMap.get(memberExpr.object);
          const objectType = checker.getTypeAtLocation(objectTsNode);

          // Use proper subtype checking if we have the database types available
          if (anyDatabaseReader || anyDatabaseWriter) {
            isDatabaseType =
              (anyDatabaseReader !== null &&
                methodName === "get" &&
                checker.isTypeAssignableTo(objectType, anyDatabaseReader)) ||
              (anyDatabaseWriter !== null &&
                checker.isTypeAssignableTo(objectType, anyDatabaseWriter));
          } else {
            // Fall back to string matching if we couldn't get the types
            const typeString = checker.typeToString(objectType);
            isDatabaseType =
              typeString.includes("DatabaseReader") ||
              typeString.includes("DatabaseWriter") ||
              typeString.includes("GenericDatabaseReader") ||
              typeString.includes("GenericDatabaseWriter");
          }
        }

        // Also check for common patterns like ctx.db. This pure-AST heuristic is
        // the fallback detector when type info is unavailable.
        const isCtxDb =
          memberExpr.object.type === AST_NODE_TYPES.MemberExpression &&
          memberExpr.object.property.type === AST_NODE_TYPES.Identifier &&
          memberExpr.object.property.name === "db";

        if (!isDatabaseType && !isCtxDb) {
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
