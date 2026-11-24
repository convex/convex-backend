import fs from "node:fs";
import path from "node:path";
import ts from "typescript";
import * as parser from "@typescript-eslint/parser";
import type { TSESTree } from "@typescript-eslint/types";
import { ASTUtils } from "@typescript-eslint/utils";
import { Linter } from "@typescript-eslint/utils/ts-eslint";
import type {
  CodePath,
  CodePathSegment,
  SourceCode,
} from "@typescript-eslint/utils/ts-eslint";

export type FunctionNode =
  | TSESTree.FunctionDeclaration
  | TSESTree.ArrowFunctionExpression
  | TSESTree.FunctionExpression;
type CallResolution =
  | { kind: "resolved"; fn: FunctionNode }
  | { kind: "shadowed" | "unloadable" | "unknown" };

type Module = { sourceCode: Readonly<SourceCode>; codePaths: CodePathTracker };

// Parsed imported files, shared across the files ESLint lints in one process
// so each is parsed once per run rather than once per linted file.
const sourceFileCache = new Map<
  string,
  { stamp: string; file: ts.SourceFile | undefined }
>();

export function isFunction(
  node: TSESTree.Node | null | undefined,
): node is FunctionNode {
  return (
    node?.type === "FunctionDeclaration" ||
    node?.type === "FunctionExpression" ||
    node?.type === "ArrowFunctionExpression"
  );
}

export function buildFunctionGraph(options: {
  filename: string;
  sourceCode: Readonly<SourceCode>;
  codePaths: CodePathTracker;
}) {
  const filename = path.resolve(options.filename);
  const services = options.sourceCode.parserServices;
  const rootSource = services?.esTreeNodeToTSNodeMap?.get(
    options.sourceCode.ast,
  );
  if (!rootSource || !ts.isSourceFile(rootSource)) {
    throw new Error(
      "Interprocedural analysis requires @typescript-eslint/parser.",
    );
  }
  // Reuse ESLint's TypeScript tree so checker symbols map back to the exact
  // nodes the rule visits, including edits that haven't been saved to disk.
  const host = ts.createCompilerHost({});
  const getSourceFile = host.getSourceFile;
  host.getSourceFile = (file, ...args) => {
    if (path.resolve(file) === filename) return rootSource;
    // Package implementations are trust boundaries, so loading their trees
    // would spend memory on functions the resolver cannot analyze.
    if (/[/\\]node_modules[/\\]/.test(file)) return undefined;
    const stat = fs.statSync(file, { throwIfNoEntry: false });
    if (!stat) return getSourceFile(file, ...args);
    const stamp = `${stat.mtimeMs}:${stat.size}`;
    const cached = sourceFileCache.get(file);
    if (cached?.stamp === stamp) return cached.file;
    const parsed = getSourceFile(file, ...args);
    sourceFileCache.set(file, { stamp, file: parsed });
    return parsed;
  };
  let program = services?.program;
  const getProgram = () =>
    (program ??= ts.createProgram({
      rootNames: [filename],
      options: {
        module: ts.ModuleKind.ESNext,
        moduleResolution: ts.ModuleResolutionKind.Bundler,
        target: ts.ScriptTarget.ESNext,
        allowJs: true,
        noLib: true,
      },
      host,
    }));
  const functions = new Set<FunctionNode>();
  const modules = new Map<ts.SourceFile, Module | null>();
  const functionModules = new WeakMap<FunctionNode, Module>();
  const root = { sourceCode: options.sourceCode, codePaths: options.codePaths };

  const register = (module: Module) => {
    for (const scope of module.sourceCode.scopeManager?.scopes ?? []) {
      if (isFunction(scope.block)) {
        functions.add(scope.block);
        functionModules.set(scope.block, module);
      }
    }
    return module;
  };
  modules.set(rootSource, register(root));

  const loadModule = (source: ts.SourceFile): Module | null => {
    if (modules.has(source)) return modules.get(source)!;
    const codePaths = new CodePathTracker();
    let sourceCode: Readonly<SourceCode> | undefined;
    try {
      const messages = new Linter({
        cwd: path.dirname(source.fileName),
      }).verify(
        source.text,
        [
          {
            files: ["**"],
            languageOptions: {
              parser,
              parserOptions: {
                // The shared options types can resolve a different TypeScript
                // release than the parser's runtime peer dependency.
                programs: [
                  getProgram() as unknown as NonNullable<
                    parser.ParserOptions["programs"]
                  >[number],
                ],
                filePath: source.fileName,
              },
            },
            linterOptions: { noInlineConfig: true },
            plugins: {
              analysis: {
                rules: {
                  capture: {
                    create(context) {
                      sourceCode = context.sourceCode;
                      return {
                        ...codePaths.listener,
                        "*": (node: TSESTree.Node) => codePaths.record(node),
                      };
                    },
                  },
                },
              },
            },
            rules: { "analysis/capture": "error" },
          },
        ],
        { filename: source.fileName },
      );
      if (messages.some((message) => message.fatal)) sourceCode = undefined;
    } catch {
      // A malformed imported module remains a local analysis boundary.
      sourceCode = undefined;
    }
    const module = sourceCode ? register({ sourceCode, codePaths }) : null;
    modules.set(source, module);
    return module;
  };

  return {
    functions: functions as ReadonlySet<FunctionNode>,
    sourceCodeForFunction(fn: FunctionNode) {
      return (functionModules.get(fn) ?? root).sourceCode;
    },
    isOnEveryReturnedPath(fn: FunctionNode, node: TSESTree.Node) {
      return (functionModules.get(fn) ?? root).codePaths.isOnEveryReturnedPath(
        fn,
        node,
      );
    },
    resolveCall(
      fn: FunctionNode,
      call: TSESTree.CallExpression,
    ): CallResolution {
      return this.resolveReference(call.callee, fn);
    },
    /**
     * Resolve a name to the function it refers to, in `fn`'s module or the
     * linted file when `fn` is omitted.
     */
    resolveReference(
      reference: TSESTree.Node,
      fn?: FunctionNode,
    ): CallResolution {
      const callee = unwrapExpression(reference);
      if (callee.type !== "Identifier") return { kind: "unknown" };
      const module = (fn && functionModules.get(fn)) ?? root;
      // Only imports need TypeScript's program, which is costly to build.
      // Globals and package imports are trust boundaries, and other names
      // resolve through scope analysis.
      const def = ASTUtils.findVariable(
        module.sourceCode.getScope(callee),
        callee,
      )?.defs[0];
      if (!def) return { kind: "unknown" };
      if (def.type !== "ImportBinding") {
        const local = resolveLocalFunction(callee, module.sourceCode);
        return local ? { kind: "resolved", fn: local } : { kind: "shadowed" };
      }
      // `import x = require("./mod")` bindings aren't followed.
      if (def.parent.type !== "ImportDeclaration") return { kind: "unknown" };
      // Without the project's program, TypeScript can't resolve path aliases
      // either, so only relative imports are followed.
      const specifier = def.parent.source.value;
      if (!services?.program && !specifier.startsWith(".")) {
        return { kind: "unknown" };
      }
      const tsNode =
        module.sourceCode.parserServices?.esTreeNodeToTSNodeMap?.get(callee);
      const checker = getProgram().getTypeChecker();
      const symbol = tsNode && checker.getSymbolAtLocation(tsNode);
      if (!symbol) return { kind: "unknown" };
      const target =
        symbol.flags & ts.SymbolFlags.Alias
          ? checker.getAliasedSymbol(symbol)
          : symbol;
      // An overloaded function's first declaration is a signature; analyze the
      // implementation.
      const declaration =
        target.declarations?.find(
          (declaration) =>
            ts.isFunctionDeclaration(declaration) && declaration.body,
        ) ?? target.valueDeclaration;
      if (!declaration) {
        // Unresolved relative imports are local failures; package imports keep
        // the rule's configured trust boundary.
        let imported: ts.Node | undefined = symbol.declarations?.[0];
        while (imported && !ts.isImportDeclaration(imported))
          imported = imported.parent;
        return {
          kind:
            imported &&
            ts.isImportDeclaration(imported) &&
            ts.isStringLiteral(imported.moduleSpecifier) &&
            imported.moduleSpecifier.text.startsWith(".")
              ? "unloadable"
              : "unknown",
        };
      }
      const source = declaration.getSourceFile();
      if (
        source.isDeclarationFile ||
        /[/\\]node_modules[/\\]/.test(source.fileName)
      )
        return { kind: "unknown" };
      const targetModule = loadModule(source);
      if (!targetModule) return { kind: "unloadable" };
      const node =
        targetModule.sourceCode.parserServices?.tsNodeToESTreeNodeMap?.get(
          declaration,
        );
      const id =
        node?.type === "VariableDeclarator" ||
        node?.type === "FunctionDeclaration"
          ? node.id
          : null;
      const variable =
        node && id?.type === "Identifier"
          ? targetModule.sourceCode.scopeManager
              ?.getDeclaredVariables(node)
              .find((variable) => variable.name === id.name)
          : undefined;
      // The checker's declaration describes the initial binding. Any other
      // write makes the callee uncertain, including a second var initializer.
      if (
        variable?.references.some(
          (reference) => reference.isWrite() && reference.identifier !== id,
        )
      )
        return { kind: "shadowed" };
      const resolved =
        node?.type === "VariableDeclarator" && node.init
          ? unwrapExpression(node.init)
          : node;
      return isFunction(resolved)
        ? { kind: "resolved", fn: resolved }
        : { kind: "shadowed" };
    },
  };
}

export type FunctionGraph = ReturnType<typeof buildFunctionGraph>;

/**
 * Resolve a name to the function it's bound to in the same file, e.g. `helper`
 * in `const helper = () => ...`. Bindings with other writes stay unresolved.
 */
export function resolveLocalFunction(
  node: TSESTree.Node,
  sourceCode: Readonly<SourceCode>,
): FunctionNode | null {
  const identifier = unwrapExpression(node);
  if (identifier.type !== "Identifier") return null;
  const variable = ASTUtils.findVariable(
    sourceCode.getScope(identifier),
    identifier,
  );
  // Overload signatures add a definition each, but only the implementation
  // runs.
  const defs = variable?.defs.filter(
    (def) => def.node.type !== "TSDeclareFunction",
  );
  const def = defs?.length === 1 ? defs[0] : undefined;
  // A parameter's definition node is the function that declares it.
  if (
    !def ||
    (def.type !== "FunctionName" && def.type !== "Variable") ||
    variable!.references.some(
      (reference) => reference.isWrite() && reference.identifier !== def.name,
    )
  ) {
    return null;
  }
  const value =
    def.node.type === "VariableDeclarator" && def.node.init
      ? unwrapExpression(def.node.init)
      : def.node;
  return isFunction(value) ? value : null;
}

/**
 * A function graph limited to the current file, for parsers without
 * TypeScript's services (e.g. Oxlint). Calls resolve through scope analysis,
 * so imported helpers stay unresolved.
 */
function buildLocalFunctionGraph(options: {
  sourceCode: Readonly<SourceCode>;
  codePaths: CodePathTracker;
}): FunctionGraph {
  const { sourceCode, codePaths } = options;
  const functions = new Set<FunctionNode>();
  for (const scope of sourceCode.scopeManager?.scopes ?? []) {
    if (isFunction(scope.block)) functions.add(scope.block);
  }
  return {
    functions,
    sourceCodeForFunction: () => sourceCode,
    isOnEveryReturnedPath: (fn, node) =>
      codePaths.isOnEveryReturnedPath(fn, node),
    resolveCall(fn, call) {
      return this.resolveReference(call.callee, fn);
    },
    resolveReference(node) {
      const fn = resolveLocalFunction(node, sourceCode);
      return fn ? { kind: "resolved", fn } : { kind: "unknown" };
    },
  };
}

/**
 * Follow calls across files when TypeScript's parser is available, and within
 * the current file otherwise.
 */
export function buildAvailableFunctionGraph(options: {
  filename: string;
  sourceCode: Readonly<SourceCode>;
  codePaths: CodePathTracker;
}): FunctionGraph {
  return options.sourceCode.parserServices?.esTreeNodeToTSNodeMap
    ? buildFunctionGraph(options)
    : buildLocalFunctionGraph(options);
}

export function solveFunctionSummaries<Summary>(options: {
  functions: ReadonlySet<FunctionNode>;
  initialSummary: () => Summary;
  analyze: (
    fn: FunctionNode,
    summaryOf: (fn: FunctionNode) => Summary | undefined,
  ) => Summary;
  equals: (left: Summary, right: Summary) => boolean;
}): Map<FunctionNode, Summary> {
  const summaries = new Map<FunctionNode, Summary>();
  for (const fn of options.functions)
    summaries.set(fn, options.initialSummary());
  // Consumers use finite monotone domains. Bound accidental oscillation.
  for (let pass = 0; pass < Math.max(1, options.functions.size * 8); pass++) {
    let changed = false;
    for (const fn of options.functions) {
      if (!summaries.has(fn)) {
        summaries.set(fn, options.initialSummary());
        changed = true;
      }
      const previous = summaries.get(fn)!;
      const next = options.analyze(fn, (callee) => summaries.get(callee));
      if (!options.equals(previous, next)) {
        summaries.set(fn, next);
        changed = true;
      }
    }
    if (!changed) return summaries;
  }
  return summaries;
}

export class CodePathTracker {
  readonly listener: {
    onCodePathStart: (codePath: CodePath, node: TSESTree.Node) => void;
    onCodePathEnd: (codePath: CodePath, node: TSESTree.Node) => void;
    onCodePathSegmentStart: (segment: CodePathSegment) => void;
    onCodePathSegmentEnd: (segment: CodePathSegment) => void;
  };

  private readonly paths = new WeakMap<TSESTree.Node, CodePath>();
  private readonly segments = new WeakMap<
    TSESTree.Node,
    Set<CodePathSegment>
  >();
  private readonly pathStack: Array<{
    node: TSESTree.Node;
    path: CodePath;
    current: Set<CodePathSegment>;
  }> = [];

  constructor() {
    this.listener = {
      onCodePathStart: (codePath, node) => {
        this.pathStack.push({ node, path: codePath, current: new Set() });
      },
      onCodePathEnd: (codePath, node) => {
        this.paths.set(node, codePath);
        this.pathStack.pop();
      },
      onCodePathSegmentStart: (segment) => {
        this.pathStack.at(-1)?.current.add(segment);
      },
      onCodePathSegmentEnd: (segment) => {
        this.pathStack.at(-1)?.current.delete(segment);
      },
    };
  }

  /** Record where `node` occurs. Call this from that node's ESLint visitor. */
  record(node: TSESTree.Node): void {
    const active = this.pathStack.at(-1);
    if (!active) return;
    this.segments.set(node, new Set(active.current));
  }

  isOnEveryReturnedPath(
    functionNode: FunctionNode,
    node: TSESTree.Node,
  ): boolean {
    const codePath = this.paths.get(functionNode);
    if (!codePath) return false;

    const guarded = new Set<CodePathSegment>();
    for (const segment of this.segments.get(node) ?? []) guarded.add(segment);
    if (!guarded.size) return false;

    const returned = new Set(
      codePath.returnedSegments.filter((segment) => segment.reachable),
    );
    if (!returned.size) return true;

    const pending = [codePath.initialSegment];
    const visited = new Set<CodePathSegment>();
    while (pending.length) {
      const segment = pending.pop()!;
      if (!segment.reachable || visited.has(segment) || guarded.has(segment)) {
        continue;
      }
      if (returned.has(segment)) return false;
      visited.add(segment);
      pending.push(...segment.nextSegments);
    }
    return true;
  }
}

export function unwrapExpression(node: TSESTree.Node): TSESTree.Node {
  let result = node;
  while (
    result.type === "AwaitExpression" ||
    result.type === "ChainExpression" ||
    result.type === "TSAsExpression" ||
    result.type === "TSSatisfiesExpression" ||
    result.type === "TSNonNullExpression" ||
    result.type === "TSTypeAssertion" ||
    result.type === "TSInstantiationExpression"
  ) {
    result =
      result.type === "AwaitExpression"
        ? result.argument
        : result.type === "ChainExpression"
          ? result.expression
          : result.expression;
  }
  return result;
}

export function asCallExpression(
  node: TSESTree.Node | null | undefined,
): TSESTree.CallExpression | null {
  if (!node) return null;
  const unwrapped = unwrapExpression(node);
  return unwrapped.type === "CallExpression" ? unwrapped : null;
}

export function getCalleeName(call: TSESTree.CallExpression): string | null {
  const callee = unwrapExpression(call.callee);
  if (callee.type === "Identifier") return callee.name;
  return callee.type === "MemberExpression"
    ? getStaticMemberName(callee)
    : null;
}

function getStaticMemberName(node: TSESTree.MemberExpression): string | null {
  if (!node.computed && node.property.type === "Identifier") {
    return node.property.name;
  }
  return node.computed && node.property.type === "Literal"
    ? String(node.property.value)
    : null;
}

function isUnshadowedGlobal(
  name: string,
  node: TSESTree.Node,
  sourceCode: Readonly<SourceCode>,
): boolean {
  try {
    const variable = ASTUtils.findVariable(sourceCode.getScope(node), name);
    return !variable || variable.defs.length === 0;
  } catch {
    // Missing scope data cannot prove that an identifier refers to a global.
    return false;
  }
}

export function getStaticGlobalPath(
  node: TSESTree.Node,
  sourceCode: Readonly<SourceCode>,
): readonly string[] | null {
  const unwrapped = unwrapExpression(node);
  if (unwrapped.type === "Identifier") {
    return isUnshadowedGlobal(unwrapped.name, unwrapped, sourceCode)
      ? [unwrapped.name]
      : null;
  }
  if (unwrapped.type !== "MemberExpression") return null;
  const object = getStaticGlobalPath(unwrapped.object, sourceCode);
  const member = getStaticMemberName(unwrapped);
  return object && member ? [...object, member] : null;
}

export function getFunctionName(fn: FunctionNode): string | null {
  if (fn.type === "FunctionDeclaration" && fn.id) return fn.id.name;
  if (
    fn.parent?.type === "VariableDeclarator" &&
    fn.parent.id.type === "Identifier"
  ) {
    return fn.parent.id.name;
  }
  return null;
}

export function getChildNodes(
  node: TSESTree.Node,
  sourceCode: Readonly<SourceCode>,
): TSESTree.Node[] {
  const result: TSESTree.Node[] = [];
  for (const key of sourceCode.visitorKeys[node.type] ?? []) {
    const value = (node as unknown as Record<string, unknown>)[key];
    const values = Array.isArray(value) ? value : [value];
    for (const child of values) {
      if (child && typeof child === "object" && "type" in child) {
        result.push(child as TSESTree.Node);
      }
    }
  }
  return result;
}

/**
 * Whether a call leaves the parameter at `index` undefined, so its default
 * runs: the argument is missing, `undefined` or `void ...`. A spread argument
 * at or before `index` might leave it undefined too.
 */
/** A function's parameters, leaving out a TypeScript `this` parameter. */
export function getRuntimeParams(fn: FunctionNode): TSESTree.Parameter[] {
  return fn.params.filter(
    (param) => param.type !== "Identifier" || param.name !== "this",
  );
}

export function usesParameterDefault(
  call: TSESTree.CallExpression | TSESTree.NewExpression,
  index: number,
  sourceCode: Readonly<SourceCode>,
): boolean {
  const args = call.arguments;
  if (args.slice(0, index + 1).some((arg) => arg.type === "SpreadElement")) {
    return true;
  }
  const arg = args[index];
  if (!arg) return true;
  const value = unwrapExpression(arg);
  if (value.type === "UnaryExpression" && value.operator === "void") {
    return true;
  }
  return (
    value.type === "Identifier" &&
    value.name === "undefined" &&
    isUnshadowedGlobal("undefined", value, sourceCode)
  );
}

export function findFirstNode<Result>(
  nodes: Iterable<TSESTree.Node>,
  visit: (node: TSESTree.Node) => Result | null,
): Result | null {
  for (const node of nodes) {
    const result = visit(node);
    if (result !== null) return result;
  }
  return null;
}
