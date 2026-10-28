import type { TSESTree } from "@typescript-eslint/utils";
import { AST_NODE_TYPES } from "@typescript-eslint/utils";
import { createRule } from "../util.js";

type MessageIds = "no-top-of-hour-crons" | "no-top-of-hour-cron-expression";

// Peel `as const` / `satisfies` / `!` / parentheses off an expression so
// wrappers around a schedule argument don't hide it from the checks below.
function unwrapExpression(expr: TSESTree.Expression): TSESTree.Expression {
  let current = expr;
  while (true) {
    switch (current.type) {
      case AST_NODE_TYPES.TSAsExpression:
      case AST_NODE_TYPES.TSSatisfiesExpression:
      case AST_NODE_TYPES.TSNonNullExpression:
      case AST_NODE_TYPES.TSTypeAssertion:
        current = current.expression;
        break;
      default:
        return current;
    }
  }
}

// Cron registration methods that take a `{ minuteUTC: ... }` schedule.
const MINUTE_UTC_CRON_METHODS = new Set([
  "hourly",
  "daily",
  "weekly",
  "monthly",
]);

// Whether the minute field of a cron expression fires at minute 0: a bare
// "0" list item or a range/step anchored at 0 ("0-5", "0/30"). Wildcard steps
// like "*/15" also hit minute 0 but the fix (a non-zero anchor like "7/15")
// is obscure enough that we leave them alone.
function minuteFieldIncludesZero(minuteField: string): boolean {
  return minuteField.split(",").some((part) => /^0+([-/]|$)/.test(part));
}

function staticStringValue(node: TSESTree.Expression): string | null {
  if (node.type === AST_NODE_TYPES.Literal && typeof node.value === "string") {
    return node.value;
  }
  if (
    node.type === AST_NODE_TYPES.TemplateLiteral &&
    node.expressions.length === 0
  ) {
    return node.quasis[0]!.value.cooked;
  }
  return null;
}

// Unwrap `as const`/`satisfies`/parens around an argument; spreads have no
// single expression to inspect.
function unwrapArgument(
  node: TSESTree.CallExpressionArgument | undefined,
): TSESTree.Expression | null {
  if (node === undefined || node.type === AST_NODE_TYPES.SpreadElement) {
    return null;
  }
  return unwrapExpression(node);
}

export const noTopOfHourCrons = createRule<[], MessageIds>({
  name: "no-top-of-hour-crons",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Warn when a cron job is scheduled at the exact top of the hour (minute 0 UTC), so background work runs at an off-peak time instead of the busiest point on the clock.",
    },
    messages: {
      "no-top-of-hour-crons":
        "This cron runs at the exact top of the hour (minuteUTC: 0), the busiest time on the clock. Apps see the most inbound traffic and scheduled work at :00. Omit minuteUTC to let Convex pick a minute and spread runs across the hour, or set a specific off-peak minute. If it must run at :00, disable this rule with `// eslint-disable-next-line @convex-dev/no-top-of-hour-crons`.",
      "no-top-of-hour-cron-expression":
        "This cron runs at the exact top of the hour (minute 0), the busiest time on the clock. Apps see the most inbound traffic and scheduled work at :00. Prefer the `.hourly()`/`.daily()` helpers without a minute so Convex can spread runs across the hour, use `.interval()` if the job doesn't need to align to the clock, or set a specific off-peak minute. If it must run at :00, disable this rule with `// eslint-disable-next-line @convex-dev/no-top-of-hour-crons`.",
    },
    schema: [],
  },
  defaultOptions: [],
  create: (context) => {
    return {
      CallExpression(node: TSESTree.CallExpression) {
        if (node.callee.type !== AST_NODE_TYPES.MemberExpression) return;
        const { property } = node.callee;
        if (property.type !== AST_NODE_TYPES.Identifier) return;

        // crons.hourly/daily/weekly/monthly("name", { minuteUTC: 0, ... }, ...)
        if (MINUTE_UTC_CRON_METHODS.has(property.name)) {
          const schedule = unwrapArgument(node.arguments[1]);
          if (schedule?.type !== AST_NODE_TYPES.ObjectExpression) return;
          for (const prop of schedule.properties) {
            if (
              prop.type !== AST_NODE_TYPES.Property ||
              prop.computed ||
              prop.key.type !== AST_NODE_TYPES.Identifier ||
              prop.key.name !== "minuteUTC"
            ) {
              continue;
            }
            const minute =
              prop.value.type === AST_NODE_TYPES.AssignmentPattern
                ? null
                : unwrapExpression(prop.value as TSESTree.Expression);
            if (minute?.type === AST_NODE_TYPES.Literal && minute.value === 0) {
              context.report({
                node: prop,
                messageId: "no-top-of-hour-crons",
              });
            }
          }
          return;
        }

        // crons.cron("name", "0 * * * *", ...)
        if (property.name === "cron") {
          const expression = unwrapArgument(node.arguments[1]);
          if (expression === null) return;
          const cronString = staticStringValue(expression);
          if (cronString === null) return;
          const fields = cronString.trim().split(/\s+/);
          if (fields.length !== 5) return;
          if (minuteFieldIncludesZero(fields[0]!)) {
            context.report({
              node: expression,
              messageId: "no-top-of-hour-cron-expression",
            });
          }
        }
      },
    };
  },
});
