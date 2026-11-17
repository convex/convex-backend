import { AnalyzedBuild } from "./model.js";
import { Sizes, sizes, sumSizes } from "./sizes.js";

/** A build's input files and what each imports, by metafile input path. */
export type InputGraph = Map<string, string[]>;

/** The input import graph, including lazily bundled dynamic imports. */
export function inputGraph(b: AnalyzedBuild): InputGraph {
  const graph: InputGraph = new Map();
  for (const [path, input] of Object.entries(b.metafile.inputs)) {
    graph.set(
      path,
      input.imports.filter((i) => !i.external).map((i) => i.path),
    );
  }
  return graph;
}

/** Code and source map bytes of each input across `outputs`, by metafile input path. */
export function sizesByInput(
  outputs: AnalyzedBuild["outputs"],
): Map<string, Sizes> {
  const bytes = new Map<string, Sizes>();
  for (const o of outputs) {
    for (const i of o.inputs) {
      bytes.set(
        i.path,
        sumSizes([
          bytes.get(i.path) ?? sizes(),
          sizes(i.bytesInOutput, i.sourceMapBytes),
        ]),
      );
    }
  }
  return bytes;
}

/** Inputs reachable from `roots` without passing through `blocked`. */
export function reachable(
  graph: InputGraph,
  roots: Iterable<string>,
  blocked: ReadonlySet<string> = new Set(),
): Set<string> {
  const seen = new Set<string>();
  const stack = [...roots].filter((r) => !blocked.has(r));
  while (stack.length > 0) {
    const next = stack.pop()!;
    if (seen.has(next)) continue;
    seen.add(next);
    for (const to of graph.get(next) ?? []) {
      if (!seen.has(to) && !blocked.has(to)) stack.push(to);
    }
  }
  return seen;
}

/** Sizes no root reaches once every input in `removed` is gone. */
export function sizesOnlyThrough(
  graph: InputGraph,
  roots: Iterable<string>,
  removed: ReadonlySet<string>,
  bytes: ReadonlyMap<string, Sizes>,
): Sizes {
  const rootList = [...roots];
  const before = reachable(graph, rootList);
  const after = reachable(graph, rootList, removed);
  return sumSizes(
    [...before]
      .filter((p) => !after.has(p))
      .map((p) => bytes.get(p) ?? sizes()),
  );
}

/**
 * The dominator tree of the inputs reachable from `roots`. An input's
 * dominated sizes are its own code plus the code of every input that the roots
 * only reach through it: what deleting its imports would remove.
 */
export type Dominators = {
  /** Immediate dominator of each reachable input; null under the roots. */
  idom: Map<string, string | null>;
  dominatedSizes: Map<string, Sizes>;
  /** Reachable inputs that import each input. */
  importers: Map<string, string[]>;
};

/**
 * Cooper, Harvey and Kennedy's iterative algorithm ("A Simple, Fast
 * Dominance Algorithm"), with a virtual root above `roots`.
 */
export function dominators(
  graph: InputGraph,
  roots: Iterable<string>,
  bytes: ReadonlyMap<string, Sizes>,
): Dominators {
  const ROOT = "\0root";
  const rootList = [...new Set(roots)];
  const succ = (n: string) => (n === ROOT ? rootList : (graph.get(n) ?? []));

  const postorder: string[] = [];
  const visited = new Set<string>([ROOT]);
  const stack: { node: string; next: number }[] = [{ node: ROOT, next: 0 }];
  while (stack.length > 0) {
    const top = stack[stack.length - 1];
    const out = succ(top.node);
    if (top.next < out.length) {
      const to = out[top.next++];
      if (!visited.has(to)) {
        visited.add(to);
        stack.push({ node: to, next: 0 });
      }
    } else {
      postorder.push(top.node);
      stack.pop();
    }
  }
  const index = new Map(postorder.map((n, i) => [n, i]));
  const rpo = [...postorder].reverse();

  const preds = new Map<string, string[]>();
  for (const n of rpo) {
    for (const to of succ(n)) {
      let list = preds.get(to);
      if (!list) preds.set(to, (list = []));
      list.push(n);
    }
  }

  const idom = new Map<string, string>([[ROOT, ROOT]]);
  const intersect = (a: string, b: string) => {
    while (a !== b) {
      while (index.get(a)! < index.get(b)!) a = idom.get(a)!;
      while (index.get(b)! < index.get(a)!) b = idom.get(b)!;
    }
    return a;
  };
  for (let changed = true; changed; ) {
    changed = false;
    for (const n of rpo) {
      if (n === ROOT) continue;
      let next: string | undefined;
      for (const p of preds.get(n) ?? []) {
        if (!idom.has(p)) continue;
        next = next === undefined ? p : intersect(p, next);
      }
      if (next !== undefined && idom.get(n) !== next) {
        idom.set(n, next);
        changed = true;
      }
    }
  }

  const dominatedSizes = new Map<string, Sizes>();
  for (const n of postorder) {
    if (n === ROOT) continue;
    dominatedSizes.set(
      n,
      sumSizes([dominatedSizes.get(n) ?? sizes(), bytes.get(n) ?? sizes()]),
    );
    const parent = idom.get(n)!;
    if (parent !== ROOT) {
      dominatedSizes.set(
        parent,
        sumSizes([
          dominatedSizes.get(parent) ?? sizes(),
          dominatedSizes.get(n)!,
        ]),
      );
    }
  }

  const result: Dominators = {
    idom: new Map(),
    dominatedSizes,
    importers: new Map(),
  };
  for (const [n, d] of idom) {
    if (n === ROOT) continue;
    const parent = d === ROOT ? null : d;
    result.idom.set(n, parent);
    result.importers.set(
      n,
      (preds.get(n) ?? []).filter((p) => p !== ROOT),
    );
  }
  return result;
}
