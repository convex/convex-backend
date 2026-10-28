import path from "path";
import fs from "fs";
import type { TSESTree } from "@typescript-eslint/utils";
import { AST_NODE_TYPES, ASTUtils } from "@typescript-eslint/utils";
import type {
  ReportFixFunction,
  RuleFix,
  RuleFixer,
  RuleContext,
} from "@typescript-eslint/utils/ts-eslint";
import { createRule } from "../util.js";

type MessageIds =
  | "no-process-env"
  | "no-process-env-whole-object"
  | "use-generated-env";

type Context = RuleContext<MessageIds, []>;

// Import sources that resolve to the generated `convex/_generated/server`
// module, whatever relative prefix and extension the project writes.
const GENERATED_SERVER_SOURCE = /(^|\/)_generated\/server(\.js|\.ts)?$/;

const IDENTIFIER = /^[A-Za-z_$][A-Za-z0-9_$]*$/;

/**
 * Everything needed to rewrite `process.env` to the typed `env` object: the
 * name `env` is bound to locally, plus the fix that creates that binding
 * (a no-op fix when the binding already exists).
 */
type EnvBinding = {
  localName: string;
  importFix: (fixer: RuleFixer) => RuleFix | null;
};

/**
 * Whether `node` is a reference to the global `process.env`, and not a
 * property of some local variable that happens to be named `process`.
 */
function isProcessEnv(
  node: TSESTree.MemberExpression,
  context: Context,
): boolean {
  if (node.object.type !== AST_NODE_TYPES.Identifier) return false;
  if (node.object.name !== "process") return false;
  if (memberKey(node) !== "env") return false;

  const variable = ASTUtils.findVariable(
    context.sourceCode.getScope(node),
    "process",
  );
  // A variable with definitions is declared in this file, so it shadows the
  // global. Globals (including `process` from an environment) have no defs.
  return variable === null || variable.defs.length === 0;
}

/** The statically known key of a `key`/`computed` pair, if there is one. */
function staticKey(key: TSESTree.Node, computed: boolean): string | null {
  if (!computed) {
    if (key.type === AST_NODE_TYPES.Identifier) return key.name;
    if (key.type === AST_NODE_TYPES.Literal && typeof key.value === "string") {
      return key.value;
    }
    return null;
  }
  if (key.type === AST_NODE_TYPES.Literal && typeof key.value === "string") {
    return key.value;
  }
  if (
    key.type === AST_NODE_TYPES.TemplateLiteral &&
    key.expressions.length === 0
  ) {
    return key.quasis[0]!.value.cooked;
  }
  return null;
}

function memberKey(member: TSESTree.MemberExpression): string | null {
  return staticKey(member.property, member.computed);
}

/** Whether this member expression is being assigned to, updated, or deleted. */
function isWriteTarget(member: TSESTree.MemberExpression): boolean {
  const parent = member.parent;
  switch (parent?.type) {
    case AST_NODE_TYPES.AssignmentExpression:
      return parent.left === member;
    case AST_NODE_TYPES.UpdateExpression:
      return true;
    case AST_NODE_TYPES.UnaryExpression:
      return parent.operator === "delete";
    default:
      return false;
  }
}

/**
 * The import specifier for `_generated/server` relative to `filename`, e.g.
 * `"./_generated/server"` for `convex/foo.ts` and `"../_generated/server"` for
 * `convex/helpers/foo.ts`. Null when the file isn't Convex code, in which case
 * we can't know where the generated code lives.
 */
function generatedServerSource(filename: string): string | null {
  const root = convexRoot(filename);
  if (root === null) return null;

  const below = path.relative(root, path.dirname(path.resolve(filename)));
  const depthBelowConvexRoot = below === "" ? 0 : below.split(path.sep).length;
  const prefix =
    depthBelowConvexRoot === 0 ? "./" : "../".repeat(depthBelowConvexRoot);
  return `${prefix}_generated/server`;
}

/**
 * The Convex functions directory the file lives under, if any: an ancestor
 * named `convex` that sits next to a `package.json`. Requiring the manifest
 * keeps a directory named `convex` further up — a folder holding several
 * checkouts, say — from making a whole unrelated project look like Convex
 * code. Like `no-import-use-node`, this only recognizes the default `convex`
 * directory, so relocated projects aren't checked.
 */
function convexRoot(filename: string): string | null {
  const segments = path.dirname(path.resolve(filename)).split(path.sep);
  for (let index = segments.length - 1; index > 0; index--) {
    if (segments[index] !== "convex") continue;
    const root = segments.slice(0, index + 1).join(path.sep);
    const manifest = path.join(path.dirname(root), "package.json");
    if (fs.existsSync(manifest)) return root;
  }
  return null;
}

// The two shapes codegen emits for `env`: `Record<string, string | undefined>`
// before the declarations are known, and a `type Env` object once they are.
const UNTYPED_ENV = /export\s+(?:declare\s+)?const\s+env\s*:\s*Record</;
const ENV_TYPE = /type\s+Env\s*=\s*\{([^}]*)\}/;
const ENV_MEMBER = /readonly\s+([A-Za-z_$][A-Za-z0-9_$]*)\s*\??\s*:/g;

type GeneratedEnv =
  // Codegen hasn't seen the declarations yet, so `env` accepts any name.
  { kind: "untyped" } | { kind: "typed"; names: Set<string> };

const generatedEnvByRoot = new Map<string, GeneratedEnv | null>();

/**
 * Which names the generated `env` object exposes — the platform-provided
 * variables plus whatever `convex.config.ts` declares. Null when that can't be
 * determined: no generated module yet, or one from a convex version that
 * predates the `env` export.
 */
function generatedEnv(filename: string): GeneratedEnv | null {
  const root = convexRoot(filename);
  if (root === null) return null;

  const cached = generatedEnvByRoot.get(root);
  if (cached !== undefined) return cached;

  const result = readGeneratedEnv(root);
  generatedEnvByRoot.set(root, result);
  return result;
}

function readGeneratedEnv(root: string): GeneratedEnv | null {
  for (const file of ["server.d.ts", "server.ts"]) {
    let source;
    try {
      source = fs.readFileSync(path.join(root, "_generated", file), "utf-8");
    } catch {
      continue;
    }
    if (UNTYPED_ENV.test(source)) return { kind: "untyped" };

    const envType = source.match(ENV_TYPE);
    if (envType) {
      const names = new Set(
        [...envType[1]!.matchAll(ENV_MEMBER)].map((member) => member[1]!),
      );
      return { kind: "typed", names };
    }
  }
  return null;
}

/**
 * Whether `env.NAME` would compile for every one of these names. Only then is
 * rewriting safe enough to apply automatically; otherwise the rewrite is
 * offered as a suggestion, since it needs a `convex.config.ts` declaration the
 * rule can't add for you.
 */
function envDeclares(filename: string, names: string[]): boolean {
  const env = generatedEnv(filename);
  if (env === null) return false;
  if (env.kind === "untyped") return true;
  return names.every((name) => env.names.has(name));
}

const NODE_MODULE_RESOLUTION =
  /"(?:moduleResolution|module)"\s*:\s*"(?:node16|nodenext)"/i;

// Keyed by the directory the search started from. tsconfigs don't change while
// ESLint runs, so one lookup per directory is enough.
const requiresExtensionsByDirectory = new Map<string, boolean>();

/**
 * Whether relative imports in this file need an explicit `.js` extension, as
 * TypeScript's `node16`/`nodenext` module resolution requires.
 */
function needsFileExtension(context: Context): boolean {
  const moduleResolution =
    context.sourceCode.parserServices?.program?.getCompilerOptions?.()
      ?.moduleResolution;
  // Any of these signals is enough. Without type-aware linting the parser
  // still reports its own default compiler options, so a `moduleResolution`
  // that isn't node16/nodenext doesn't rule out a tsconfig that says otherwise.
  return (
    // ts.ModuleResolutionKind.Node16 is 3 and NodeNext is 99. Hardcoded rather
    // than imported so the rule keeps working without type-aware linting.
    moduleResolution === 3 ||
    moduleResolution === 99 ||
    tsconfigRequiresExtensions(path.dirname(path.resolve(context.filename))) ||
    context.sourceCode.ast.body.some(
      (statement) =>
        statement.type === AST_NODE_TYPES.ImportDeclaration &&
        typeof statement.source.value === "string" &&
        statement.source.value.startsWith(".") &&
        statement.source.value.endsWith(".js"),
    )
  );
}

function tsconfigRequiresExtensions(fromDirectory: string): boolean {
  const cached = requiresExtensionsByDirectory.get(fromDirectory);
  if (cached !== undefined) return cached;

  let result = false;
  let directory = fromDirectory;
  for (;;) {
    const tsconfig = path.join(directory, "tsconfig.json");
    if (fs.existsSync(tsconfig)) {
      result = configRequiresExtensions(tsconfig, 3);
      break;
    }
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }

  requiresExtensionsByDirectory.set(fromDirectory, result);
  return result;
}

// Matched with a regex rather than parsed, since tsconfigs allow comments and
// trailing commas that `JSON.parse` rejects. `depth` bounds the extends chain.
function configRequiresExtensions(file: string, depth: number): boolean {
  let source;
  try {
    source = fs.readFileSync(file, "utf-8");
  } catch {
    return false;
  }
  if (NODE_MODULE_RESOLUTION.test(source)) return true;
  if (depth === 0) return false;

  const extended = source.match(/"extends"\s*:\s*"(\.[^"]*)"/);
  if (!extended) return false;
  let target = path.resolve(path.dirname(file), extended[1]!);
  if (!target.endsWith(".json")) target += ".json";
  return configRequiresExtensions(target, depth - 1);
}

/** Null when the `env` binding can't be introduced, so there's no rewrite. */
function resolveEnvBinding(
  node: TSESTree.Node,
  context: Context,
): EnvBinding | null {
  const program = context.sourceCode.ast;
  const generatedImports = program.body.filter(
    (statement): statement is TSESTree.ImportDeclaration =>
      statement.type === AST_NODE_TYPES.ImportDeclaration &&
      typeof statement.source.value === "string" &&
      GENERATED_SERVER_SOURCE.test(statement.source.value) &&
      statement.importKind !== "type",
  );

  // Already importing `env` from the generated module: reuse that binding.
  for (const declaration of generatedImports) {
    for (const specifier of declaration.specifiers) {
      if (
        specifier.type === AST_NODE_TYPES.ImportSpecifier &&
        specifier.imported.type === AST_NODE_TYPES.Identifier &&
        specifier.imported.name === "env" &&
        specifier.importKind !== "type"
      ) {
        return { localName: specifier.local.name, importFix: () => null };
      }
    }
  }

  // The name `env` is taken by something else here, so we'd shadow or collide.
  if (
    ASTUtils.findVariable(context.sourceCode.getScope(node), "env") !== null
  ) {
    return null;
  }

  // Add `env` to an existing named-value import from the generated module.
  const existing = generatedImports.find(
    (declaration) =>
      declaration.specifiers.length > 0 &&
      declaration.specifiers.every(
        (specifier) => specifier.type === AST_NODE_TYPES.ImportSpecifier,
      ),
  );
  if (existing) {
    const lastSpecifier = existing.specifiers[existing.specifiers.length - 1]!;
    return {
      localName: "env",
      importFix: (fixer) => fixer.insertTextAfter(lastSpecifier, ", env"),
    };
  }

  const source = generatedServerSource(context.filename);
  if (source === null) return null;

  const extension = needsFileExtension(context) ? ".js" : "";
  const statement = `import { env } from "${source}${extension}";\n`;
  const firstImport = program.body.find(
    (statement) => statement.type === AST_NODE_TYPES.ImportDeclaration,
  );
  return {
    localName: "env",
    importFix: (fixer) =>
      firstImport
        ? fixer.insertTextBefore(firstImport, statement)
        : fixer.insertTextBeforeRange([0, 0], statement),
  };
}

export const noProcessEnv = createRule<[], MessageIds>({
  name: "no-process-env",
  meta: {
    type: "suggestion",
    docs: {
      description:
        "Prefer the typed `env` object from `_generated/server` over `process.env` in Convex code.",
    },
    messages: {
      "no-process-env":
        "Avoid `process.env` in Convex code. Declare this environment variable in `convex/convex.config.ts` and read it from the typed `env` object exported by `_generated/server` instead: https://docs.convex.dev/production/environment-variables#declaring",
      "no-process-env-whole-object":
        "Avoid `process.env` in Convex code. Declare your environment variables in `convex/convex.config.ts` and read them from the typed `env` object exported by `_generated/server` instead: https://docs.convex.dev/production/environment-variables#declaring",
      "use-generated-env":
        "Read it from the typed `env` object, and declare it in `convex/convex.config.ts`",
    },
    schema: [],
    fixable: "code",
    hasSuggestions: true,
  },
  defaultOptions: [],
  create: (context) => {
    // `process.env` is the right thing to read in frontend and other Node.js
    // code, so only files under a `convex` directory are checked.
    if (convexRoot(context.filename) === null) return {};

    // Codegen writes `export const env = process.env` in the generated server
    // module, so linting generated files would report the very fix we suggest.
    if (context.filename.includes("_generated")) return {};

    return {
      MemberExpression(node: TSESTree.MemberExpression) {
        if (!isProcessEnv(node, context)) return;

        const parent = node.parent;

        // process.env.FOO / process.env["FOO"]
        if (
          parent?.type === AST_NODE_TYPES.MemberExpression &&
          parent.object === node
        ) {
          const key = memberKey(parent);
          const binding =
            key !== null && !isWriteTarget(parent)
              ? resolveEnvBinding(node, context)
              : null;
          if (key === null || binding === null) {
            context.report({ node, messageId: "no-process-env" });
            return;
          }
          const access = IDENTIFIER.test(key)
            ? `${binding.localName}.${key}`
            : `${binding.localName}[${JSON.stringify(key)}]`;
          report(context, node, [key], (fixer) =>
            compact([
              binding.importFix(fixer),
              fixer.replaceText(parent, access),
            ]),
          );
          return;
        }

        // const { FOO, BAR } = process.env
        const keys =
          parent?.type === AST_NODE_TYPES.VariableDeclarator &&
          parent.init === node &&
          parent.id.type === AST_NODE_TYPES.ObjectPattern
            ? destructuredKeys(parent.id)
            : null;
        if (keys !== null) {
          const binding = resolveEnvBinding(node, context);
          if (binding === null) {
            context.report({ node, messageId: "no-process-env" });
            return;
          }
          report(context, node, keys, (fixer) =>
            compact([
              binding.importFix(fixer),
              fixer.replaceText(node, binding.localName),
            ]),
          );
          return;
        }

        // Passing, spreading, or otherwise using the whole `process.env`
        // object: the typed `env` object holds only the declared variables, so
        // there's no rewrite to offer.
        context.report({ node, messageId: "no-process-env-whole-object" });
      },
    };
  },
});

function compact(fixes: (RuleFix | null)[]): RuleFix[] {
  return fixes.filter((fix): fix is RuleFix => fix !== null);
}

/**
 * The names a destructuring pattern pulls out of `process.env`, or null when
 * the pattern has no mechanical equivalent: an empty pattern, a computed or
 * dynamic key, or a rest element, which would collect the undeclared variables
 * too — and those aren't on the typed `env` object.
 */
function destructuredKeys(pattern: TSESTree.ObjectPattern): string[] | null {
  const keys: string[] = [];
  for (const property of pattern.properties) {
    if (property.type !== AST_NODE_TYPES.Property) return null;
    const key = staticKey(property.key, property.computed);
    if (key === null) return null;
    keys.push(key);
  }
  return keys.length > 0 ? keys : null;
}

/**
 * Report the rewrite as an autofix when every name it reads is on the
 * generated `env` object, and as a suggestion otherwise: the rewrite is still
 * what you want, but it only compiles once you declare the variable in
 * `convex.config.ts`, which a fix can't do from here.
 */
function report(
  context: Context,
  node: TSESTree.Node,
  names: string[],
  fix: ReportFixFunction,
): void {
  if (envDeclares(context.filename, names)) {
    context.report({ node, messageId: "no-process-env", fix });
  } else {
    context.report({
      node,
      messageId: "no-process-env",
      suggest: [{ messageId: "use-generated-env", fix }],
    });
  }
}
