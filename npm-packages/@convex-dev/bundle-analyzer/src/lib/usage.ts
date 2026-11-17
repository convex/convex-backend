import { AnalyzedBuild, ownerLabel } from "./model.js";
import { inputGraph } from "./graph.js";

/** Which function modules and files of yours use some inputs of a build. */
export type TargetUse = {
  /**
   * Function modules (by source path) whose code includes the inputs' code.
   * esbuild splits chunks after tree-shaking, so this leaves out modules
   * that import the inputs without using what they export.
   */
  usedBy: string[];
  /**
   * Your files that import the inputs, directly or through other packages,
   * with those packages. Only files that appear just in modules in `usedBy`,
   * which are the ones whose imports keep the code, when there are any.
   */
  files: Map<string, Set<string>>;
};

export function targetUse(
  b: AnalyzedBuild,
  isTarget: (inputPath: string) => boolean,
): TargetUse {
  const byPath = new Map(b.outputs.map((o) => [o.path, o]));
  const withCode = new Set(
    b.outputs
      .filter((o) =>
        o.inputs.some((i) => i.bytesInOutput > 0 && isTarget(i.path)),
      )
      .map((o) => o.path),
  );
  const usedBy = new Set<string>();
  for (const entry of b.outputs) {
    if (!entry.entryPoint) continue;
    const seen = new Set<string>();
    const stack = [entry.path];
    while (stack.length > 0) {
      const p = stack.pop()!;
      if (seen.has(p)) continue;
      seen.add(p);
      if (withCode.has(p)) {
        usedBy.add(entry.entryPoint);
        break;
      }
      const o = byPath.get(p);
      stack.push(...(o?.imports ?? []), ...(o?.dynamicImports ?? []));
    }
  }

  const roots = new Set(
    b.outputs.flatMap((o) => (o.entryPoint ? [o.entryPoint] : [])),
  );
  const importers = new Map<string, string[]>();
  for (const [from, imports] of inputGraph(b)) {
    for (const to of imports) {
      importers.set(to, [...(importers.get(to) ?? []), from]);
    }
  }
  const rootsAbove = (start: string[]) => {
    const found = new Set<string>();
    const seen = new Set(start);
    const stack = [...start];
    while (stack.length > 0) {
      const p = stack.pop()!;
      if (roots.has(p)) found.add(p);
      for (const from of importers.get(p) ?? []) {
        if (!seen.has(from)) {
          seen.add(from);
          stack.push(from);
        }
      }
    }
    return found;
  };
  const targets = [...importers.keys(), ...roots].filter(isTarget);

  // Your files reaching the inputs through packages only.
  const candidates = new Map<string, Set<string>>();
  const seen = new Set(targets);
  const stack = [...targets];
  while (stack.length > 0) {
    const p = stack.pop()!;
    for (const from of importers.get(p) ?? []) {
      if (isTarget(from)) continue;
      const a = b.attribution.get(from);
      if (a?.origin === "app") {
        const via = candidates.get(from) ?? new Set();
        const at = b.attribution.get(p);
        if (!isTarget(p) && at?.packageName) via.add(ownerLabel(at));
        candidates.set(from, via);
      } else if (!seen.has(from)) {
        seen.add(from);
        stack.push(from);
      }
    }
  }
  const inUsing = [...candidates.keys()].map((f) => {
    const modules = [...rootsAbove([f])];
    return {
      file: f,
      some: modules.some((m) => usedBy.has(m)),
      only: modules.length > 0 && modules.every((m) => usedBy.has(m)),
    };
  });
  const only = inUsing.filter((c) => c.only);
  const chosen = only.length > 0 ? only : inUsing.filter((c) => c.some);
  const files = new Map(
    chosen
      .map((c) => [c.file, candidates.get(c.file)!] as const)
      .sort(([x], [y]) => x.localeCompare(y)),
  );
  return {
    usedBy: [...usedBy].sort(),
    files,
  };
}
