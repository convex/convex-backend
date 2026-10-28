import type { TSESTree } from "@typescript-eslint/utils";
import { ASTUtils, AST_NODE_TYPES } from "@typescript-eslint/utils";
import type { SourceCode } from "@typescript-eslint/utils/ts-eslint";
import { createRule } from "../util.js";

type MessageIds = "prefix-index" | "duplicate-index";

// Peel `as const` / `satisfies` / `!` / type assertions off an expression so
// wrappers around an index config don't hide it from the checks below.
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

function staticStringValue(node: TSESTree.Node): string | null {
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

function unwrapArgument(
  node: TSESTree.CallExpressionArgument | undefined,
): TSESTree.Expression | null {
  if (node === undefined || node.type === AST_NODE_TYPES.SpreadElement) {
    return null;
  }
  return unwrapExpression(node);
}

function importedName(specifier: TSESTree.ImportSpecifier): string {
  return specifier.imported.type === AST_NODE_TYPES.Identifier
    ? specifier.imported.name
    : specifier.imported.value;
}

/**
 * Whether an identifier resolves to an import specifier that `matches`
 * accepts. The binding is resolved from the identifier's own scope outwards,
 * so a parameter or local of the same name shadows the import and doesn't
 * match. Modules aren't resolved: any module may re-export `defineTable`, and
 * the name is distinctive enough that following the chain isn't worth the cost.
 */
function resolvesToImport(
  sourceCode: Readonly<SourceCode>,
  node: TSESTree.Identifier,
  matches: (definition: TSESTree.Node) => boolean,
): boolean {
  const variable = ASTUtils.findVariable(sourceCode.getScope(node), node);
  return variable !== null && variable.defs.some((def) => matches(def.node));
}

function isDefineTableCall(
  node: TSESTree.CallExpression,
  sourceCode: Readonly<SourceCode>,
): boolean {
  const callee = node.callee;
  if (callee.type === AST_NODE_TYPES.Identifier) {
    // `import { defineTable } from ...`, possibly aliased.
    return resolvesToImport(
      sourceCode,
      callee,
      (definition) =>
        definition.type === AST_NODE_TYPES.ImportSpecifier &&
        importedName(definition) === "defineTable",
    );
  }
  // `import * as server from ...` used as `server.defineTable(...)`.
  return (
    callee.type === AST_NODE_TYPES.MemberExpression &&
    !callee.computed &&
    callee.object.type === AST_NODE_TYPES.Identifier &&
    callee.property.type === AST_NODE_TYPES.Identifier &&
    callee.property.name === "defineTable" &&
    resolvesToImport(
      sourceCode,
      callee.object,
      (definition) =>
        definition.type === AST_NODE_TYPES.ImportNamespaceSpecifier,
    )
  );
}

/**
 * Whether every key of an index config is statically readable. A spread or a
 * computed key can supply `staged` or `fields` in a way the checks below can't
 * see, so such a config is left alone rather than assumed to be unstaged.
 */
function hasOnlyStaticKeys(object: TSESTree.ObjectExpression): boolean {
  return object.properties.every(
    (prop) => prop.type === AST_NODE_TYPES.Property && !prop.computed,
  );
}

function propertyNamed(
  object: TSESTree.ObjectExpression,
  name: string,
): TSESTree.Property | null {
  // Later properties win, matching object literal evaluation.
  let found: TSESTree.Property | null = null;
  for (const prop of object.properties) {
    if (
      prop.type === AST_NODE_TYPES.Property &&
      !prop.computed &&
      ((prop.key.type === AST_NODE_TYPES.Identifier &&
        prop.key.name === name) ||
        staticStringValue(prop.key) === name)
    ) {
      found = prop;
    }
  }
  return found;
}

type IndexInfo = {
  name: string;
  fields: string[];
  // Location covering just `.index(...)`, not the chain to its left.
  loc: TSESTree.SourceLocation;
};

/**
 * The name and fields of an `.index()` call, or `null` if this index should be
 * skipped: it's staged, or its name/fields aren't statically known.
 */
function parseIndexCall(node: TSESTree.CallExpression): IndexInfo | null {
  const nameArg = unwrapArgument(node.arguments[0]);
  if (nameArg === null) return null;
  const name = staticStringValue(nameArg);
  if (name === null) return null;

  let fieldsExpr = unwrapArgument(node.arguments[1]);
  if (fieldsExpr === null) return null;

  if (fieldsExpr.type === AST_NODE_TYPES.ObjectExpression) {
    if (!hasOnlyStaticKeys(fieldsExpr)) return null;
    const staged = propertyNamed(fieldsExpr, "staged");
    if (staged !== null) {
      const value =
        staged.value.type === AST_NODE_TYPES.AssignmentPattern
          ? null
          : unwrapExpression(staged.value as TSESTree.Expression);
      // Only a literal `staged: false` is known not to be staged.
      if (value?.type !== AST_NODE_TYPES.Literal || value.value !== false) {
        return null;
      }
    }
    const fields = propertyNamed(fieldsExpr, "fields");
    if (
      fields === null ||
      fields.value.type === AST_NODE_TYPES.AssignmentPattern
    ) {
      return null;
    }
    fieldsExpr = unwrapExpression(fields.value as TSESTree.Expression);
  }

  if (fieldsExpr.type !== AST_NODE_TYPES.ArrayExpression) return null;

  const fields: string[] = [];
  for (const element of fieldsExpr.elements) {
    if (element === null) return null;
    const field = staticStringValue(element);
    if (field === null) return null;
    fields.push(field);
  }
  if (fields.length === 0) return null;

  const property = (node.callee as TSESTree.MemberExpression).property;
  return {
    name,
    fields,
    loc: { start: property.loc.start, end: node.loc.end },
  };
}

// Climb past `as`/`satisfies`/`!` wrappers around a link in a builder chain,
// which sit between the call and the member access that continues the chain.
function skipWrappingParents(node: TSESTree.Node): TSESTree.Node {
  let current = node;
  while (true) {
    const parent: TSESTree.Node | undefined = current.parent;
    switch (parent?.type) {
      case AST_NODE_TYPES.TSAsExpression:
      case AST_NODE_TYPES.TSSatisfiesExpression:
      case AST_NODE_TYPES.TSNonNullExpression:
      case AST_NODE_TYPES.TSTypeAssertion:
        current = parent;
        break;
      default:
        return current;
    }
  }
}

/**
 * Every `.index()` call chained onto `defineTable(...)`, in source order.
 * Walks up past `.searchIndex()`, `.vectorIndex()` and `.staged()` links,
 * which return the same table definition but don't define db indexes.
 *
 * Only chains written directly on the `defineTable(...)` call are followed. A
 * table held in a variable and indexed in a separate statement would need
 * cross-statement tracking, so its indexes go unchecked rather than being
 * attributed to the wrong table.
 */
function chainedIndexes(defineTable: TSESTree.CallExpression): IndexInfo[] {
  const indexes: IndexInfo[] = [];
  let current: TSESTree.Node = defineTable;
  while (true) {
    current = skipWrappingParents(current);
    const member: TSESTree.Node | undefined = current.parent;
    if (
      member?.type !== AST_NODE_TYPES.MemberExpression ||
      member.object !== current ||
      member.computed
    ) {
      return indexes;
    }
    const call: TSESTree.Node | undefined = member.parent;
    if (
      call?.type !== AST_NODE_TYPES.CallExpression ||
      call.callee !== member
    ) {
      return indexes;
    }
    if (
      member.property.type === AST_NODE_TYPES.Identifier &&
      member.property.name === "index"
    ) {
      const index = parseIndexCall(call);
      if (index !== null) {
        indexes.push(index);
      }
    }
    current = call;
  }
}

function isPrefix(shorter: string[], longer: string[]): boolean {
  return (
    shorter.length <= longer.length &&
    shorter.every((field, i) => field === longer[i])
  );
}

// The covering index may be defined far from the report, so name it with its
// fields. The reported index needs only its name: its fields are on the line
// the report points at.
function describe(index: IndexInfo): string {
  const fields = index.fields.map((f) => JSON.stringify(f)).join(", ");
  return `"${index.name}" ([${fields}])`;
}

export const noDuplicateIndexes = createRule<[], MessageIds>({
  name: "no-duplicate-indexes",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Disallow database indexes whose fields are a prefix of another index's fields, since the longer index selects the same documents for any query over those leading fields.",
    },
    messages: {
      "prefix-index":
        "Index {{duplicate}} indexes the start of index {{covering}}, which answers the same queries. Every index slows down writes and adds storage, so delete {{duplicate}} and query {{coveringName}} instead. Keep both only if you need the different sort order: {{duplicate}} sorts matching rows by `_creationTime`, {{coveringName}} sorts them by {{nextField}}. To keep it, add `// eslint-disable-next-line @convex-dev/no-duplicate-indexes`.",
      "duplicate-index":
        "Index {{duplicate}} indexes the same fields as index {{covering}}, so the two behave identically in every query while each slows down writes and adds storage. Delete {{duplicate}} and query {{coveringName}} instead.",
    },
    schema: [],
  },
  defaultOptions: [],
  create: (context) => {
    return {
      CallExpression(node: TSESTree.CallExpression) {
        if (!isDefineTableCall(node, context.sourceCode)) return;
        const indexes = chainedIndexes(node);

        for (const [i, duplicate] of indexes.entries()) {
          // Report each duplicate index once, against the first index that
          // covers it. Identical field lists are attributed to the later
          // index so a pair produces one report rather than two.
          const covering = indexes.find(
            (other, j) =>
              j !== i &&
              isPrefix(duplicate.fields, other.fields) &&
              (other.fields.length > duplicate.fields.length || j < i),
          );
          if (covering === undefined) continue;
          context.report({
            loc: duplicate.loc,
            messageId:
              covering.fields.length === duplicate.fields.length
                ? "duplicate-index"
                : "prefix-index",
            data: {
              duplicate: `"${duplicate.name}"`,
              covering: describe(covering),
              coveringName: `"${covering.name}"`,
              // The first field the covering index sorts by that the reported
              // index doesn't, i.e. what it orders matching rows by instead of
              // `_creationTime`. Unused by the duplicate-index message.
              nextField: JSON.stringify(
                covering.fields[duplicate.fields.length],
              ),
            },
          });
        }
      },
    };
  },
});
