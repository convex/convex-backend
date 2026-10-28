import type { TSESTree } from "@typescript-eslint/types";
import { CONVEX_REGISTRARS, createRule, isEntryPoint } from "../util.js";

/**
 * Rule to enforce using object syntax for Convex functions instead of the older function syntax
 */
export const noOldRegisteredFunctionSyntax = createRule({
  name: "no-old-registered-function-syntax",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Don't use the non-object Convex function syntax. It's harder to add validation rules.",
    },
    messages: {
      "use-object-syntax":
        "Use the object syntax for registered Convex queries, mutations, and actions.",
    },
    schema: [],
    fixable: "code",
  },
  defaultOptions: [],
  create: (context) => {
    const { filename } = context;
    // Skip generated files
    const isGenerated = filename.includes("_generated");
    const entry = isEntryPoint(filename);
    if (isGenerated || !entry) {
      return {};
    }

    /**
     * Check if the function has a second parameter (args parameter)
     * that would indicate it expects arguments
     */
    function hasFunctionArgs(
      fn: TSESTree.ArrowFunctionExpression | TSESTree.FunctionExpression,
    ): boolean {
      return fn.params.length >= 2;
    }

    return {
      // Check variable declarations for exports that use the old syntax
      VariableDeclarator(node) {
        // Only interested in export declarations
        const parentDecl = node.parent;
        if (!parentDecl) return;

        const exportDecl = parentDecl.parent;
        if (
          exportDecl?.type !== "ExportNamedDeclaration" &&
          parentDecl.parent?.parent?.type !== "ExportNamedDeclaration"
        ) {
          return;
        }

        // Check if it's a call to a registrar with a function argument
        if (
          node.init?.type === "CallExpression" &&
          node.init.callee.type === "Identifier" &&
          CONVEX_REGISTRARS.includes(node.init.callee.name) &&
          node.init.arguments.length === 1 &&
          (node.init.arguments[0].type === "ArrowFunctionExpression" ||
            node.init.arguments[0].type === "FunctionExpression")
        ) {
          const functionArg = node.init.arguments[0] as
            | TSESTree.ArrowFunctionExpression
            | TSESTree.FunctionExpression;

          // Report the issue
          context.report({
            node: node.init,
            messageId: "use-object-syntax",
            fix: (fixer) => {
              // Check if the function has a second parameter (args)
              const hasArgsParam = hasFunctionArgs(functionArg);

              // Create object syntax replacement
              let fixText = "{\n";

              // We only add empty args if there's no second parameter
              // If there is a second parameter, we leave args undefined for the no-missing-args-validator
              // rule to handle it correctly later
              if (!hasArgsParam) {
                fixText += "  args: {},\n";
              }

              // Get the original function text without the outer parentheses
              const originalFunctionText =
                context.sourceCode.getText(functionArg);

              // Add the handler property with the original function
              fixText += `  handler: ${originalFunctionText}`;

              fixText += "\n}";

              return fixer.replaceText(functionArg, fixText);
            },
          });
        }
      },
    };
  },
});
