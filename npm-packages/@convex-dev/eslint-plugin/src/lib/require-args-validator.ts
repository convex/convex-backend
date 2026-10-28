import type { TSESTree } from "@typescript-eslint/types";
import {
  CONVEX_REGISTRARS,
  createRule,
  getRegisteredFunction,
} from "../util.js";
import type {
  ReportFixFunction,
  RuleContext,
} from "@typescript-eslint/utils/ts-eslint";

/**
 * Helper function to check if an object expression has an args property
 */
function hasArgsProperty(objectExpr: TSESTree.ObjectExpression): boolean {
  return objectExpr.properties.some(
    (prop) =>
      prop.type === "Property" &&
      prop.key.type === "Identifier" &&
      prop.key.name === "args",
  );
}

/**
 * Helper function to check if a handler function has a non-empty second parameter (args parameter)
 */
function handlerHasArgsParameter(
  handler: TSESTree.ArrowFunctionExpression | TSESTree.FunctionExpression,
): boolean {
  if (handler.params.length < 2) {
    return false;
  }

  // Ignore empty objects
  const secondParam = handler.params[1];
  if (
    secondParam.type === "ObjectPattern" &&
    secondParam.properties.length === 0
  ) {
    return false;
  }

  return true;
}

/**
 * Helper function to create a fix for missing args property
 */
function createArgsFix(
  context: RuleContext<string, unknown[]>,
  objectArg: TSESTree.ObjectExpression,
): ReportFixFunction {
  return (fixer) => {
    const objectText = context.sourceCode.getText(objectArg);
    const firstBracePos = objectText.indexOf("{");

    if (firstBracePos === -1) return null;

    const insertPos = objectArg.range[0] + firstBracePos + 1;
    return fixer.insertTextAfterRange([insertPos, insertPos], "\n  args: {},");
  };
}

type MessageIds = "missing-empty-args" | "missing-args";

type Options = [
  {
    ignoreUnusedArguments: boolean;
  },
];

/**
 * Rule to enforce that every registered Convex function has an args property
 */
export const requireArgsValidator = createRule<Options, MessageIds>({
  name: "require-args-validator",
  meta: {
    type: "suggestion",
    docs: {
      description: "Require argument validators (`args`) in Convex functions.",
    },
    messages: {
      "missing-empty-args": "Convex function is missing args validator.",
      "missing-args":
        "Convex function is missing args validator but has parameter. Add appropriate args validator.",
    },
    schema: [
      {
        type: "object",
        properties: {
          ignoreUnusedArguments: {
            type: "boolean",
            description:
              "If true, don’t require args validator when function doesn’t use args parameter",
          },
        },
        additionalProperties: false,
      },
    ],
    defaultOptions: [{ ignoreUnusedArguments: false }],
    fixable: "code",
  },
  defaultOptions: [{ ignoreUnusedArguments: false }],
  create: (context, options) => {
    const { ignoreUnusedArguments } = options[0];

    const { filename } = context;

    // Generated files don’t define functions, so we skip them to avoid unnecessary work
    const isGenerated = filename.includes("_generated");
    if (isGenerated) {
      return {};
    }

    return {
      VariableDeclarator(node) {
        const parentDecl = node.parent;
        if (!parentDecl) return;

        // In an export?
        const exportDecl = parentDecl.parent;
        if (
          exportDecl?.type !== "ExportNamedDeclaration" &&
          parentDecl.parent?.parent?.type !== "ExportNamedDeclaration"
        ) {
          return;
        }

        // Convex function declaration?
        const registered = getRegisteredFunction(node.init, CONVEX_REGISTRARS);
        if (!registered) return;

        // Old function argument syntax?
        if (!registered.objectArg && registered.handler) {
          const handler = registered.handler;
          if (handlerHasArgsParameter(handler)) {
            context.report({
              node: registered.call,
              messageId: "missing-args",
              // Not fixable since we don’t know the type
            });
            return;
          }

          if (!ignoreUnusedArguments) {
            context.report({
              node: registered.call,
              messageId: "missing-empty-args",
              fix: (fixer) => {
                let fixText = "{\n";
                fixText += "  args: {},\n";

                // Get the original function text without the outer parentheses
                const originalFunctionText =
                  context.sourceCode.getText(handler);

                // Add the handler property with the original function
                fixText += `  handler: ${originalFunctionText}`;

                fixText += "\n}";

                return fixer.replaceText(handler, fixText);
              },
            });
          }

          return;
        }

        // New syntax with object argument
        if (registered.objectArg) {
          const objectArg = registered.objectArg;
          if (hasArgsProperty(objectArg)) {
            return;
          }

          const handlerProp = registered.handler;
          const handlerHasArgs =
            handlerProp && handlerHasArgsParameter(handlerProp);

          if (!handlerHasArgs && ignoreUnusedArguments) return;

          context.report({
            node: objectArg,
            messageId: handlerHasArgs ? "missing-args" : "missing-empty-args",
            fix: handlerHasArgs ? undefined : createArgsFix(context, objectArg),
          });
        }
      },
    };
  },
});
