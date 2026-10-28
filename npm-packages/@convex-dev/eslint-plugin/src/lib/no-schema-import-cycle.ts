import path from "path";
import fs from "fs";
import type { TSESTree } from "@typescript-eslint/utils";
import { AST_NODE_TYPES } from "@typescript-eslint/utils";
import { createRule } from "../util.js";

type MessageIds = "no-schema-import-cycle";

// Stop traversing rather than walking a pathologically large import graph.
// A schema's import closure is normally a handful of files.
const MAX_FILES_TRAVERSED = 500;

const RESOLVE_EXTENSIONS = [".ts", ".tsx", ".js", ".jsx", ".mts", ".mjs"];

// Under NodeNext, TypeScript sources are imported through the extension they
// compile to, so `./schema.js` is how a `.ts` file refers to `schema.ts`.
const TS_EXTENSIONS_FOR_JS: Record<string, string[]> = {
  ".js": [".ts", ".tsx"],
  ".jsx": [".tsx"],
  ".mjs": [".mts"],
  ".cjs": [".cts"],
};

// Matches `import ... from "spec"`, `export ... from "spec"` and bare
// `import "spec"`, capturing whether the statement is type-only. The clause
// before `from` excludes quotes and semicolons so a match can't run away past
// the end of its own statement. Dynamic `import("spec")` is deliberately not
// matched: it resolves after the importing module has finished evaluating, so
// it can't produce the uninitialized binding this rule is about.
const IMPORT_RE =
  /\b(?:import|export)\s+(type\s+)?(?:[^"';]*?\bfrom\s*)?["']([^"']+)["']/g;

const LINE_COMMENT_RE = /\/\/[^\n]*/g;
const BLOCK_COMMENT_RE = /\/\*[\s\S]*?\*\//g;

// Implement basic module resolution for relative paths only. Like
// `no-import-use-node`, this doesn't handle path aliases or package exports.
function isFile(filePath: string): boolean {
  return fs.existsSync(filePath) && fs.statSync(filePath).isFile();
}

function resolveFile(filePath: string): string | null {
  if (isFile(filePath)) return filePath;

  const extension = path.extname(filePath);
  const withoutExtension = filePath.slice(
    0,
    filePath.length - extension.length,
  );
  for (const tsExtension of TS_EXTENSIONS_FOR_JS[extension] ?? []) {
    const candidate = `${withoutExtension}${tsExtension}`;
    if (isFile(candidate)) return candidate;
  }

  for (const ext of RESOLVE_EXTENSIONS) {
    const candidate = `${filePath}${ext}`;
    if (isFile(candidate)) return candidate;
  }

  if (fs.existsSync(filePath) && fs.statSync(filePath).isDirectory()) {
    for (const ext of RESOLVE_EXTENSIONS) {
      const indexPath = path.join(filePath, `index${ext}`);
      if (isFile(indexPath)) return indexPath;
    }
  }
  return null;
}

// The relative specifiers a file imports at evaluation time. Read with a
// regex rather than a parser: the plugin doesn't depend on one, and an
// over- or under-matched specifier here only affects whether we find a cycle.
function relativeImportsOf(filePath: string): string[] {
  let source: string;
  try {
    source = fs.readFileSync(filePath, { encoding: "utf-8" });
  } catch {
    return [];
  }
  const code = source
    .replace(BLOCK_COMMENT_RE, "")
    .replace(LINE_COMMENT_RE, "");
  const specifiers: string[] = [];
  for (const match of code.matchAll(IMPORT_RE)) {
    const isTypeOnly = match[1] !== undefined;
    const specifier = match[2]!;
    if (isTypeOnly || !specifier.startsWith(".")) continue;
    specifiers.push(specifier);
  }
  return specifiers;
}

// Breadth-first search for the shortest import path from `fromFile` to
// `targetFile`, so the message can show how the schema reaches this file.
function importChain(fromFile: string, targetFile: string): string[] | null {
  const queue: string[][] = [[fromFile]];
  const seen = new Set([fromFile]);
  let traversed = 0;
  while (queue.length > 0) {
    const chain = queue.shift()!;
    const current = chain[chain.length - 1]!;
    if (++traversed > MAX_FILES_TRAVERSED) return null;
    for (const specifier of relativeImportsOf(current)) {
      const resolved = resolveFile(
        path.resolve(path.dirname(current), specifier),
      );
      if (resolved === null || seen.has(resolved)) continue;
      if (resolved === targetFile) return [...chain, resolved];
      seen.add(resolved);
      queue.push([...chain, resolved]);
    }
  }
  return null;
}

function isSchemaFile(filePath: string): boolean {
  return path.parse(filePath).name === "schema";
}

// Convex projects can relocate their functions directory with the `functions`
// field in convex.json; like `no-import-use-node`, this only recognizes the
// default `convex` directory, so relocated projects aren't checked.
function isInConvexDir(filePath: string): boolean {
  return path.dirname(filePath).split(path.sep).includes("convex");
}

// A reference under a `typeof` type query (`type Schema = typeof schema`) is
// erased at compile time and never runs. The scope manager marks these as
// value references, since `typeof` queries the value binding, so check the
// syntax tree instead.
function isInTypeQuery(node: TSESTree.Node): boolean {
  let current: TSESTree.Node | undefined | null = node.parent;
  while (current !== undefined && current !== null) {
    if (current.type === AST_NODE_TYPES.TSTypeQuery) return true;
    current = current.parent;
  }
  return false;
}

// Whether a reference runs when the module is evaluated, as opposed to later
// when a function body or per-instance field initializer is invoked. Only the
// former can observe the half-initialized binding from an import cycle.
function isDeferred(node: TSESTree.Node): boolean {
  // `Program.parent` is null, so this terminates at the top of the tree.
  let child: TSESTree.Node = node;
  let current: TSESTree.Node | undefined | null = node.parent;
  while (current !== undefined && current !== null) {
    switch (current.type) {
      case AST_NODE_TYPES.FunctionDeclaration:
      case AST_NODE_TYPES.FunctionExpression:
      case AST_NODE_TYPES.ArrowFunctionExpression:
        return true;
      case AST_NODE_TYPES.PropertyDefinition:
      case AST_NODE_TYPES.AccessorProperty:
        // An instance field initializer runs at construction time. Static
        // initializers (like static blocks and computed keys) run when the
        // class is defined, so keep walking to see where the class sits.
        if (!current.static && current.value === child) return true;
        break;
      default:
        break;
    }
    child = current;
    current = current.parent;
  }
  return false;
}

export const noSchemaImportCycle = createRule<[], MessageIds>({
  name: "no-schema-import-cycle",
  meta: {
    type: "problem",
    docs: {
      description:
        "Disallow using the schema value in a file that the schema imports, which leaves the schema undefined at import time",
    },
    messages: {
      "no-schema-import-cycle":
        '`{{schema}}` imports this file ({{chain}}), so `{{name}}` is still undefined while this module is evaluated. Using it here fails at import time with `Cannot read properties of undefined`. Use `v.id("tableName")` instead of `{{name}}.id()`, or move this code to a file the schema doesn\'t import.',
    },
    schema: [],
  },
  defaultOptions: [],
  create: (context) => {
    const filename = path.resolve(context.filename);
    if (!isInConvexDir(filename) || isSchemaFile(filename)) return {};
    const currentDir = path.dirname(filename);

    return {
      ImportDeclaration(node: TSESTree.ImportDeclaration) {
        // Type-only imports are erased before anything runs.
        if (node.importKind === "type") return;
        if (typeof node.source.value !== "string") return;
        const specifier = node.source.value;
        if (!specifier.startsWith(".")) return;

        const resolved = resolveFile(path.resolve(currentDir, specifier));
        if (resolved === null || !isSchemaFile(resolved)) return;

        // Report the uses of the binding rather than the import itself: a
        // reference inside a function body runs after both modules have
        // finished evaluating, so it's safe even inside a cycle.
        const evaluationTimeUses: (
          | TSESTree.Identifier
          | TSESTree.JSXIdentifier
        )[] = [];
        for (const variable of context.sourceCode.getDeclaredVariables(node)) {
          for (const reference of variable.references) {
            // References in type positions are erased at compile time and
            // never run.
            if (!reference.isValueReference) continue;
            if (isInTypeQuery(reference.identifier)) continue;
            if (!isDeferred(reference.identifier)) {
              evaluationTimeUses.push(reference.identifier);
            }
          }
        }
        if (evaluationTimeUses.length === 0) return;

        const chain = importChain(resolved, filename);
        if (chain === null) return;

        const schemaName = path.basename(resolved);
        const chainText = chain.map((file) => path.basename(file)).join(" → ");
        for (const identifier of evaluationTimeUses) {
          context.report({
            node: identifier,
            messageId: "no-schema-import-cycle",
            data: {
              schema: schemaName,
              chain: chainText,
              name: identifier.name,
            },
          });
        }
      },
    };
  },
});
