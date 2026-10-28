import path from "node:path";
import ts from "typescript";
import * as parser from "@typescript-eslint/parser";
import type { TSESTree } from "@typescript-eslint/types";
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
export type CallResolution =
  | { kind: "resolved"; fn: FunctionNode }
  | { kind: "shadowed" | "unloadable" | "unknown" };

type Module = { sourceCode: Readonly<SourceCode>; codePaths: CodePathTracker };

function isFunction(node: TSESTree.Node | undefined): node is FunctionNode {
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
    return getSourceFile(file, ...args);
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
      const callee = unwrapExpression(call.callee);
      if (callee.type !== "Identifier") return { kind: "unknown" };
      const module = functionModules.get(fn) ?? root;
      const tsNode =
        module.sourceCode.parserServices?.esTreeNodeToTSNodeMap?.get(callee);
      const checker = getProgram().getTypeChecker();
      const symbol = tsNode && checker.getSymbolAtLocation(tsNode);
      if (!symbol) return { kind: "unknown" };
      const target =
        symbol.flags & ts.SymbolFlags.Alias
          ? checker.getAliasedSymbol(symbol)
          : symbol;
      const declaration = target.valueDeclaration;
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
  if (
    callee.type === "MemberExpression" &&
    !callee.computed &&
    callee.property.type === "Identifier"
  ) {
    return callee.property.name;
  }
  return null;
}
