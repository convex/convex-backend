import type { TSESTree } from "@typescript-eslint/types";
import type { RegisteredFunction } from "../util.js";
import {
  PUBLIC_CONVEX_REGISTRARS,
  createRule,
  getRegisteredFunction,
  isEntryPoint,
  unwrapTSExpression,
} from "../util.js";

/**
 * Bare name of the callee (`requireUser`, or `require` for `authz.require`),
 * or null when it isn’t a plain identifier or method. A bare `require(…)` is
 * a CommonJS import, never an access check.
 */
function getCalleeName(callExpr: TSESTree.CallExpression): string | null {
  const { callee } = callExpr;
  if (callee.type === "Identifier") {
    return callee.name === "require" ? null : callee.name;
  }
  if (
    callee.type === "MemberExpression" &&
    !callee.computed &&
    callee.property.type === "Identifier"
  ) {
    return callee.property.name;
  }
  return null;
}

/**
 * Unwrap `await someCall()` and TypeScript-only wrappers (e.g.
 * `(await someCall())!`) down to `someCall()`.
 */
function asCallExpression(
  expr: TSESTree.Node | null | undefined,
): TSESTree.CallExpression | null {
  if (!expr) return null;
  let unwrapped = unwrapTSExpression(expr);
  while (unwrapped.type === "AwaitExpression") {
    unwrapped = unwrapTSExpression(unwrapped.argument);
  }
  return unwrapped.type === "CallExpression" ? unwrapped : null;
}

/**
 * Is this expression a call to an access-control function?
 */
function isAccessControlCall(
  expr: TSESTree.Node | null | undefined,
  pattern: RegExp,
): boolean {
  const call = asCallExpression(expr);
  if (!call) return false;
  const name = getCalleeName(call);
  return name !== null && pattern.test(name);
}

/**
 * Is there a call to an access-control function anywhere inside this
 * expression? Used for `if` conditions, where boolean checks show up negated
 * and combined: `if (!(await canEdit(ctx)) || isBanned) throw …`.
 */
function containsAccessControlCall(
  expr: TSESTree.Node | null | undefined,
  pattern: RegExp,
): boolean {
  if (!expr) return false;
  if (isAccessControlCall(expr, pattern)) return true;

  const unwrapped = unwrapTSExpression(expr);
  switch (unwrapped.type) {
    case "AwaitExpression":
    case "UnaryExpression":
      return containsAccessControlCall(unwrapped.argument, pattern);
    case "LogicalExpression":
    case "BinaryExpression":
      return (
        containsAccessControlCall(unwrapped.left, pattern) ||
        containsAccessControlCall(unwrapped.right, pattern)
      );
    default:
      return false;
  }
}

/**
 * Does this top-level statement of a handler body call an access-control
 * function?
 */
function isAccessControlStatement(
  statement: TSESTree.Statement,
  pattern: RegExp,
): boolean {
  if (statement.type === "ExpressionStatement") {
    return isAccessControlCall(statement.expression, pattern);
  }
  if (statement.type === "VariableDeclaration") {
    return statement.declarations.some((declarator) =>
      isAccessControlCall(declarator.init, pattern),
    );
  }
  if (statement.type === "ReturnStatement") {
    return isAccessControlCall(statement.argument, pattern);
  }
  if (statement.type === "IfStatement") {
    // A boolean check acts as access control when it decides whether the
    // handler goes on, e.g. `if (!(await canEdit(ctx))) throw …`
    return containsAccessControlCall(statement.test, pattern);
  }
  return false;
}

function hasAccessControl(
  handler: TSESTree.ArrowFunctionExpression | TSESTree.FunctionExpression,
  pattern: RegExp,
): boolean {
  if (handler.body.type !== "BlockStatement") {
    // Expression body, e.g. `(ctx) => requireUser(ctx)`
    return isAccessControlCall(handler.body, pattern);
  }
  return handler.body.body.some((statement) =>
    isAccessControlStatement(statement, pattern),
  );
}

type MessageIds = "missing-access-control";

/**
 * Each prefix must end at a word boundary, so `canView` and `authz.can` match
 * but `canister` doesn’t. `has` also requires a suffix: a bare `has()` is
 * usually a `Map`/`Set` lookup.
 */
export const DEFAULT_ACCESS_CONTROL_PATTERN =
  "^(require|assert|check|ensure|can)([A-Z_]|$)|^has[A-Z_]";

type Options = [
  {
    // A string, not a RegExp: ESLint validates options against `meta.schema`
    // (JSON Schema), where a RegExp fails as `{}`. Compiled in `create()`.
    pattern: string;
  },
];

/**
 * Rule to enforce that every public Convex function starts by calling an
 * access-control function such as `requireIsAdmin(ctx)`.
 */
export const requireAccessControl = createRule<Options, MessageIds>({
  name: "require-access-control",
  meta: {
    type: "problem",
    docs: {
      description:
        "Require an access control check in public Convex functions.",
    },
    messages: {
      "missing-access-control":
        "Anyone on the internet can call this Convex function. Add a top-level call to an access control function {{expected}}, or use an internal registrar (e.g. `internalMutation`) if this function shouldn’t be callable from outside.",
    },
    schema: [
      {
        type: "object",
        properties: {
          pattern: {
            type: "string",
            description:
              "Regular expression matching the names of access control functions",
          },
        },
        additionalProperties: false,
      },
    ],
    defaultOptions: [{ pattern: DEFAULT_ACCESS_CONTROL_PATTERN }],
    // Not fixable: the rule can’t know which check belongs here.
  },
  defaultOptions: [{ pattern: DEFAULT_ACCESS_CONTROL_PATTERN }],
  create: (context, options) => {
    if (!isEntryPoint(context.filename)) {
      return {};
    }

    const { pattern: patternSource } = options[0];

    let pattern: RegExp;
    try {
      pattern = new RegExp(patternSource);
    } catch (e) {
      throw new Error(
        `Invalid \`pattern\` option for the require-access-control rule: ${patternSource} is not a valid regular expression (${e instanceof Error ? e.message : String(e)})`,
      );
    }

    // The default pattern is too dense to show to someone who just wants to
    // know what to write, so describe it by example instead.
    const expected =
      patternSource === DEFAULT_ACCESS_CONTROL_PATTERN
        ? "named like `requireUser`, `checkAccess`, `canEditNote` or `hasRole`"
        : `whose name matches \`${patternSource}\``;

    const check = (registered: RegisteredFunction) => {
      if (
        registered.handler &&
        !hasAccessControl(registered.handler, pattern)
      ) {
        context.report({
          node: registered.callee,
          messageId: "missing-access-control",
          data: { expected },
        });
      }
    };

    // Public functions declared as `const name = mutation(…)`, kept until
    // Program:exit so that `export { name }` statements anywhere in the file
    // can mark them as exported.
    const declaredFunctions = new Map<string, RegisteredFunction>();
    const exportedNames = new Set<string>();

    return {
      VariableDeclarator(node) {
        if (node.id.type !== "Identifier") return;
        const registered = getRegisteredFunction(
          node.init,
          PUBLIC_CONVEX_REGISTRARS,
        );
        if (!registered) return;

        declaredFunctions.set(node.id.name, registered);
        if (node.parent.parent?.type === "ExportNamedDeclaration") {
          exportedNames.add(node.id.name);
        }
      },
      ExportNamedDeclaration(node) {
        // `export { name }` (`export … from "module"` re-exports don’t
        // reference local declarations)
        if (node.source) return;
        for (const specifier of node.specifiers) {
          if (specifier.local.type === "Identifier") {
            exportedNames.add(specifier.local.name);
          }
        }
      },
      ExportDefaultDeclaration(node) {
        const declaration = unwrapTSExpression(node.declaration);
        if (declaration.type === "Identifier") {
          exportedNames.add(declaration.name);
          return;
        }
        const registered = getRegisteredFunction(
          declaration,
          PUBLIC_CONVEX_REGISTRARS,
        );
        if (registered) check(registered);
      },
      "Program:exit"() {
        for (const name of exportedNames) {
          const registered = declaredFunctions.get(name);
          if (registered) check(registered);
        }
      },
    };
  },
});
