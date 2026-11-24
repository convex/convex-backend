import type { TSESTree } from "@typescript-eslint/types";
import type { SourceCode } from "@typescript-eslint/utils/ts-eslint";
import {
  buildAvailableFunctionGraph,
  CodePathTracker,
  findFirstNode,
  getChildNodes,
  getFunctionName,
  getRuntimeParams,
  getStaticGlobalPath,
  isFunction,
  resolveLocalFunction,
  solveFunctionSummaries,
  unwrapExpression,
  usesParameterDefault,
  type FunctionGraph,
  type FunctionNode,
} from "./interprocedural-analysis.js";

export type NondeterminismSummary = {
  chain: readonly string[] | null;
  node: TSESTree.Node | null;
  path: ReadonlySet<FunctionNode>;
};

type SummaryOf = (fn: FunctionNode) => NondeterminismSummary | undefined;

const TEMPORAL_NOW_METHODS = new Set([
  "instant",
  "plainDateISO",
  "plainDateTimeISO",
  "plainTimeISO",
  "zonedDateTimeISO",
]);

function emptySummary(): NondeterminismSummary {
  return { chain: null, node: null, path: new Set() };
}

function withoutGlobalThis(path: readonly string[]): readonly string[] {
  return path[0] === "globalThis" ? path.slice(1) : path;
}

type NondeterministicSource = {
  /** How the source appears in diagnostics, e.g. `Date.now()`. */
  label: string;
};

const RANDOM_SOURCES = new Set([
  "Math.random",
  "crypto.randomUUID",
  "crypto.getRandomValues",
]);

/** Whether a finding's chain ends in a random source rather than the clock. */
export function isRandomChain(chain: readonly string[]): boolean {
  const source = chain[chain.length - 1];
  return RANDOM_SOURCES.has(source.replace(/\(\)$/, ""));
}

/**
 * Recognize a direct read from the runtime's clock or random number
 * generator. Shadowed globals are ignored. This needs scope data but not type
 * information.
 */
function classifyNondeterministicCall(
  node: TSESTree.CallExpression | TSESTree.NewExpression,
  sourceCode: Readonly<SourceCode>,
): NondeterministicSource | null {
  const path = getStaticGlobalPath(node.callee, sourceCode);
  const globalPath = path && withoutGlobalThis(path);
  if (
    node.type === "NewExpression" &&
    globalPath?.length === 1 &&
    globalPath[0] === "Date" &&
    node.arguments.length === 0
  ) {
    return { label: "new Date()" };
  }
  if (node.type !== "CallExpression" || !globalPath) return null;
  if (globalPath.length === 1 && globalPath[0] === "Date") {
    return { label: "Date()" };
  }
  const joined = globalPath.join(".");
  if (RANDOM_SOURCES.has(joined)) {
    return { label: `${joined}()` };
  }
  if (
    joined === "Date.now" ||
    joined === "performance.now" ||
    (globalPath.length === 3 &&
      globalPath[0] === "Temporal" &&
      globalPath[1] === "Now" &&
      TEMPORAL_NOW_METHODS.has(globalPath[2]))
  ) {
    return { label: `${joined}()` };
  }
  return null;
}

function extendSummary(
  caller: FunctionNode,
  callee: FunctionNode,
  call: TSESTree.CallExpression | TSESTree.NewExpression,
  summary: NondeterminismSummary | undefined,
): NondeterminismSummary | null {
  if (!summary?.chain || summary.path.has(caller)) return null;
  return {
    chain: [`${getFunctionName(callee) ?? "helper"}()`, ...summary.chain],
    node: call,
    path: new Set([caller, ...summary.path]),
  };
}

function findNondeterminismInNodes(
  fn: FunctionNode,
  nodes: Iterable<TSESTree.Node>,
  graph: FunctionGraph,
  summaryOf: SummaryOf,
  descendIntoFunctions = false,
): NondeterminismSummary | null {
  return findFirstNode(nodes, (node) =>
    findNondeterminism(fn, node, graph, summaryOf, descendIntoFunctions),
  );
}

// The call itself: a direct read, or a callee or named callback that reads.
// Its callee and argument expressions are checked separately.
function findNondeterminismAtCall(
  fn: FunctionNode,
  node: TSESTree.CallExpression | TSESTree.NewExpression,
  graph: FunctionGraph,
  summaryOf: SummaryOf,
): NondeterminismSummary | null {
  const direct = classifyNondeterministicCall(
    node,
    graph.sourceCodeForFunction(fn),
  );
  if (direct) {
    return { chain: [direct.label], node, path: new Set([fn]) };
  }
  // An inline callee like `(() => Date.now())()` runs immediately. Constructors
  // aren't followed, but their arguments are checked like a call's.
  const inline = unwrapExpression(node.callee);
  const resolution =
    node.type !== "CallExpression"
      ? ({ kind: "unknown" } as const)
      : isFunction(inline)
        ? ({ kind: "resolved", fn: inline } as const)
        : graph.resolveCall(fn, node);
  if (resolution.kind === "resolved") {
    const called =
      extendSummary(fn, resolution.fn, node, summaryOf(resolution.fn)) ??
      findNondeterminismInDefaults(fn, resolution.fn, node, graph, summaryOf);
    if (called) return called;
  }
  // Callbacks are treated as invoked, whether inline or named like
  // `[1].map(getNow)`.
  for (const argument of callArguments(node)) {
    const named = graph.resolveReference(argument, fn);
    if (named.kind !== "resolved") continue;
    // Its arguments are unknown, so any of its defaults might run.
    const called =
      extendSummary(fn, named.fn, node, summaryOf(named.fn)) ??
      findNondeterminismInDefaults(fn, named.fn, node, graph, summaryOf, false);
    if (called) return called;
  }
  return null;
}

function callArguments(
  node: TSESTree.CallExpression | TSESTree.NewExpression,
): TSESTree.Node[] {
  return node.arguments.map((argument) =>
    argument.type === "SpreadElement" ? argument.argument : argument,
  );
}

function findNondeterminismInCall(
  fn: FunctionNode,
  node: TSESTree.CallExpression | TSESTree.NewExpression,
  graph: FunctionGraph,
  summaryOf: SummaryOf,
): NondeterminismSummary | null {
  return (
    findNondeterminismAtCall(fn, node, graph, summaryOf) ??
    (isFunction(unwrapExpression(node.callee))
      ? null
      : findNondeterminism(fn, node.callee, graph, summaryOf)) ??
    findNondeterminismInNodes(fn, callArguments(node), graph, summaryOf, true)
  );
}

const expandingDefaults = new Set<FunctionNode>();

// A function's summary leaves out its parameter defaults, since they only run
// when a call leaves the parameter undefined. When the arguments are unknown,
// every default might run.
function findNondeterminismInDefaults(
  fn: FunctionNode,
  callee: FunctionNode,
  call: TSESTree.CallExpression | TSESTree.NewExpression,
  graph: FunctionGraph,
  summaryOf: SummaryOf,
  argumentsKnown = true,
): NondeterminismSummary | null {
  // Defaults that call back into the same function, like `a = (t = a()) => t`,
  // are only expanded once.
  if (expandingDefaults.has(callee)) return null;
  expandingDefaults.add(callee);
  try {
    const sourceCode = graph.sourceCodeForFunction(fn);
    for (const [index, param] of getRuntimeParams(callee).entries()) {
      if (
        param.type !== "AssignmentPattern" ||
        (argumentsKnown && !usesParameterDefault(call, index, sourceCode))
      ) {
        continue;
      }
      const found = findNondeterminism(callee, param.right, graph, summaryOf);
      const called = extendSummary(fn, callee, call, found ?? undefined);
      if (called) return called;
    }
    return null;
  } finally {
    expandingDefaults.delete(callee);
  }
}

function functionChildren(
  fn: FunctionNode,
  sourceCode: Readonly<SourceCode>,
): TSESTree.Node[] {
  return getChildNodes(fn, sourceCode).map((child) =>
    child.type === "AssignmentPattern" && fn.params.includes(child)
      ? child.left
      : child,
  );
}

function findNondeterminism(
  fn: FunctionNode,
  node: TSESTree.Node,
  graph: FunctionGraph,
  summaryOf: SummaryOf,
  descendIntoFunction = false,
): NondeterminismSummary | null {
  if (node !== fn && isFunction(node) && !descendIntoFunction) return null;
  if (node.type === "CallExpression" || node.type === "NewExpression") {
    return findNondeterminismInCall(fn, node, graph, summaryOf);
  }
  const sourceCode = graph.sourceCodeForFunction(fn);
  return findNondeterminismInNodes(
    fn,
    node === fn
      ? functionChildren(fn, sourceCode)
      : getChildNodes(node, sourceCode),
    graph,
    summaryOf,
  );
}

/**
 * Every nondeterministic call in a handler, so a disable comment on one
 * doesn't hide the others. Helpers are still summarized by their first call,
 * reported where the handler calls them.
 */
function collectHandlerNondeterminism(
  fn: FunctionNode,
  graph: FunctionGraph,
  summaryOf: SummaryOf,
): NondeterminismSummary[] {
  const sourceCode = graph.sourceCodeForFunction(fn);
  const found: NondeterminismSummary[] = [];
  const visit = (node: TSESTree.Node, descendIntoFunction = false) => {
    if (node !== fn && isFunction(node) && !descendIntoFunction) return;
    if (node.type === "CallExpression" || node.type === "NewExpression") {
      const atCall = findNondeterminismAtCall(fn, node, graph, summaryOf);
      if (atCall) found.push(atCall);
      // An inline callee's body is covered by the call's own summary.
      if (!isFunction(unwrapExpression(node.callee))) visit(node.callee);
      for (const argument of callArguments(node)) visit(argument, true);
      return;
    }
    const children =
      node === fn
        ? functionChildren(fn, sourceCode)
        : getChildNodes(node, sourceCode);
    for (const child of children) visit(child);
  };
  visit(fn, true);
  // Convex passes a handler `ctx` and `args`, so defaults of any later
  // parameters always run.
  for (const param of getRuntimeParams(fn).slice(2)) {
    if (param.type === "AssignmentPattern") visit(param.right);
  }
  return found;
}

function summariesEqual(
  left: NondeterminismSummary,
  right: NondeterminismSummary,
): boolean {
  return (
    left.chain?.join("\0") === right.chain?.join("\0") &&
    left.path.size === right.path.size &&
    [...left.path].every((fn) => right.path.has(fn))
  );
}

/** Analyze nondeterministic operations through local and relative-import calls. */
export function analyzeNondeterminism(options: {
  filename: string;
  sourceCode: Readonly<SourceCode>;
  codePaths: CodePathTracker;
  /** Handler expressions: inline functions or names of functions. */
  roots: ReadonlyArray<TSESTree.Node>;
}) {
  const graph = buildAvailableFunctionGraph(options);
  const resolveRoot = (node: TSESTree.Node) => {
    if (isFunction(node)) return node;
    // Scope analysis also covers shorthand `{ handler }`, which TypeScript
    // resolves to the property rather than the variable.
    const local = resolveLocalFunction(node, options.sourceCode);
    if (local) return local;
    const resolution = graph.resolveReference(node);
    return resolution.kind === "resolved" ? resolution.fn : null;
  };
  // Resolve before solving so handlers imported from other files are loaded
  // into the graph.
  const roots = new Map(options.roots.map((node) => [node, resolveRoot(node)]));
  // Only functions reachable from the handlers are analyzed, so modules are
  // loaded only when a handler's calls lead to them.
  const reachable = new Set<FunctionNode>();
  for (const fn of roots.values()) if (fn) reachable.add(fn);
  const rootFunctions = new Set(reachable);
  const summaries = solveFunctionSummaries({
    functions: reachable,
    initialSummary: emptySummary,
    analyze: (fn, summaryOf) => {
      const reach: SummaryOf = (callee) => {
        reachable.add(callee);
        return summaryOf(callee);
      };
      // A handler's whole body is walked so every function its calls reach
      // is solved, not just those before its first finding.
      return (
        (rootFunctions.has(fn)
          ? collectHandlerNondeterminism(fn, graph, reach)[0]
          : findNondeterminism(fn, fn, graph, reach, true)) ?? emptySummary()
      );
    },
    equals: summariesEqual,
  });
  const summaryOf: SummaryOf = (fn) => summaries.get(fn);
  return {
    /** Each nondeterministic call in the handler, with where to report it. */
    findingsFor(node: TSESTree.Node): NondeterminismSummary[] {
      const fn = roots.get(node);
      if (!fn) return [];
      // An imported handler's operations are in another file, so report the
      // first at the handler reference.
      if (graph.sourceCodeForFunction(fn) !== options.sourceCode) {
        const summary = summaries.get(fn);
        return summary?.chain ? [{ ...summary, node: null }] : [];
      }
      return collectHandlerNondeterminism(fn, graph, summaryOf);
    },
  };
}
