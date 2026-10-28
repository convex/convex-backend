import {
  AnalyzedBuild,
  AnalyzedInput,
  AnalyzedOutput,
  CODE_GLUE_NAME,
  MAP_OVERHEAD_NAME,
  SourceOrigin,
  UNMATCHED_SOURCES_NAME,
  displayInputPath,
  distinctExternalPackages,
  inputBytes,
  ownerLabel,
  unmatchedSourceMapBytes,
} from "./model.js";

export type TreeNode = {
  name: string;
  bytes: number;
  kind:
    | "root"
    | "external"
    | "overhead"
    | "component"
    | "build"
    | "module"
    | "owner"
    | "file"
    | "dir"
    | "glue";
  origin?: SourceOrigin | undefined;
  /** For owner nodes: the package name (or label) without version or install directory. */
  ownerName?: string | undefined;
  /** Metafile input path for file nodes, used to look up import chains. */
  inputPath?: string | undefined;
  buildId?: string | undefined;
  children: TreeNode[];
};

export type Grouping = "component" | "package";

/**
 * Code, source maps, and distinct installed external packages. `component` groups
 * component → build → module → package → file, with unattributed bytes as
 * `glue` nodes under each module (unmatched source map sources as children of
 * one of them); `package` groups package → build → file with unattributed
 * bytes in a separate branch. External package sizes are installed estimates.
 * Zero-byte nodes are dropped.
 */
export function tree(builds: AnalyzedBuild[], grouping: Grouping): TreeNode {
  const root =
    grouping === "component" ? componentTree(builds) : packageTree(builds);
  const external = distinctExternalPackages(builds).filter(
    (pkg) => pkg.diskBytes !== null && pkg.diskBytes > 0,
  );
  if (external.length > 0) {
    root.children.push({
      name: "external packages (installed estimate)",
      kind: "external",
      bytes: 0,
      children: external.map((pkg) => ({
        name: `${pkg.name}@${pkg.version}`,
        kind: "external",
        bytes: pkg.diskBytes!,
        ownerName: pkg.name,
        children: [],
      })),
    });
  }
  finalize(root);
  return root;
}

/**
 * Keys owners by install directory as well as `name@version`, since distinct
 * install directories are distinct copies even at the same version. Copies
 * that share a `name@version` are named by their directory too.
 */
function ownerIdentity(
  builds: AnalyzedBuild[],
): (i: AnalyzedInput) => { key: string; name: string; ownerName: string } {
  const dirsByLabel = new Map<string, Set<string>>();
  for (const b of builds) {
    for (const o of b.outputs) {
      for (const i of o.inputs) {
        if (i.packageDir === null) continue;
        const label = ownerLabel(i);
        dirsByLabel.set(
          label,
          (dirsByLabel.get(label) ?? new Set()).add(i.packageDir),
        );
      }
    }
  }
  return (i) => {
    const label = ownerLabel(i);
    const copies = dirsByLabel.get(label)?.size ?? 0;
    return {
      key: `${label}\0${i.packageDir ?? ""}`,
      name:
        copies > 1 && i.packageDir !== null
          ? `${label} in ${i.packageDir.replace(/^(\.\.\/)+/, "")}`
          : label,
      ownerName: i.packageName ?? label,
    };
  };
}

function componentTree(builds: AnalyzedBuild[]): TreeNode {
  const identify = ownerIdentity(builds);
  const root: TreeNode = {
    name: "bundle",
    bytes: 0,
    kind: "root",
    children: [],
  };
  const componentNodes = new Map<string, TreeNode>();
  for (const b of builds) {
    let comp = componentNodes.get(b.component.label);
    if (!comp) {
      comp = {
        name: b.component.label,
        bytes: 0,
        kind: "component",
        children: [],
      };
      componentNodes.set(b.component.label, comp);
      root.children.push(comp);
    }
    const buildNode: TreeNode = {
      name: b.kind,
      bytes: 0,
      kind: "build",
      buildId: b.id,
      children: [],
    };
    comp.children.push(buildNode);
    for (const o of b.outputs) {
      const moduleNode: TreeNode = {
        name: o.path,
        bytes: 0,
        kind: "module",
        buildId: b.id,
        children: [],
      };
      buildNode.children.push(moduleNode);
      const owners = new Map<string, TreeNode>();
      for (const i of o.inputs) {
        const bytes = inputBytes(i);
        if (bytes === 0) continue;
        const { key, name, ownerName } = identify(i);
        let owner = owners.get(key);
        if (!owner) {
          owner = {
            name,
            bytes: 0,
            kind: "owner",
            origin: i.origin,
            ownerName,
            buildId: b.id,
            children: [],
          };
          owners.set(key, owner);
          moduleNode.children.push(owner);
        }
        insertFile(owner, i, bytes, b.id);
      }
      moduleNode.children.push(...outputGlue(o, b.id));
    }
  }
  return root;
}

function outputGlue(o: AnalyzedOutput, buildId: string): TreeNode[] {
  const entries: [string, number, TreeNode[]][] = [
    [CODE_GLUE_NAME, o.unattributedBytes, []],
    [MAP_OVERHEAD_NAME, o.sourceMapOverheadBytes, []],
    [
      UNMATCHED_SOURCES_NAME,
      unmatchedSourceMapBytes(o),
      [...o.sourceMapUnmatchedBytesBySource]
        .filter(([, bytes]) => bytes > 0)
        .map(([source, bytes]) => ({
          name: source,
          bytes,
          kind: "glue",
          origin: "virtual",
          buildId,
          children: [],
        })),
    ],
  ];
  return entries
    .filter(([, bytes]) => bytes > 0)
    .map(([name, bytes, children]) => ({
      name,
      bytes,
      children,
      kind: "glue",
      origin: "virtual",
      buildId,
    }));
}

function packageTree(builds: AnalyzedBuild[]): TreeNode {
  const identify = ownerIdentity(builds);
  const root: TreeNode = {
    name: "bundle",
    bytes: 0,
    kind: "root",
    children: [],
  };
  const overhead: TreeNode = {
    name: "unattributed bytes",
    bytes: 0,
    kind: "overhead",
    children: [],
  };
  const owners = new Map<string, TreeNode>();
  for (const b of builds) {
    for (const o of b.outputs) {
      const glue = outputGlue(o, b.id);
      if (glue.length > 0) {
        overhead.children.push({
          name: `${b.id} / ${o.path}`,
          bytes: 0,
          kind: "overhead",
          children: glue,
        });
      }

      for (const i of o.inputs) {
        const bytes = inputBytes(i);
        if (bytes === 0) continue;
        const { key, name, ownerName } = identify(i);
        let owner = owners.get(key);
        if (!owner) {
          owner = {
            name,
            bytes: 0,
            kind: "owner",
            origin: i.origin,
            ownerName,
            children: [],
          };
          owners.set(key, owner);
          root.children.push(owner);
        }
        let buildNode = owner.children.find((c) => c.buildId === b.id);
        if (!buildNode) {
          buildNode = {
            name: b.id,
            bytes: 0,
            kind: "build",
            origin: i.origin,
            buildId: b.id,
            children: [],
          };
          owner.children.push(buildNode);
        }
        insertFile(buildNode, i, bytes, b.id);
      }
    }
  }
  if (overhead.children.length > 0) root.children.push(overhead);
  return root;
}

function insertFile(
  parent: TreeNode,
  input: AnalyzedInput,
  bytes: number,
  buildId: string,
) {
  const segments = displayInputPath(input).split("/");
  let node = parent;
  for (const segment of segments.slice(0, -1)) {
    let dir = node.children.find((c) => c.kind === "dir" && c.name === segment);
    if (!dir) {
      dir = {
        name: segment,
        bytes: 0,
        kind: "dir",
        origin: input.origin,
        children: [],
      };
      node.children.push(dir);
    }
    node = dir;
  }
  const fileName = segments[segments.length - 1];
  const existing = node.children.find(
    (c) => c.kind === "file" && c.name === fileName,
  );
  if (existing) {
    existing.bytes += bytes;
    return;
  }
  node.children.push({
    name: fileName,
    bytes,
    kind: "file",
    origin: input.origin,
    inputPath: input.path,
    buildId,
    children: [],
  });
}

function finalize(node: TreeNode): number {
  collapseSingleDirs(node);
  if (node.children.length > 0) {
    node.bytes = node.children.reduce((n, c) => n + finalize(c), 0);
    node.children = node.children.filter((c) => c.bytes > 0);
    node.children.sort((a, b) => b.bytes - a.bytes);
  }
  return node.bytes;
}

/** Merges chains of single-child directories (`a/b/c`) into one node. */
function collapseSingleDirs(node: TreeNode) {
  for (let idx = 0; idx < node.children.length; idx++) {
    let child = node.children[idx];
    while (
      child.kind === "dir" &&
      child.children.length === 1 &&
      child.children[0].kind === "dir"
    ) {
      const only = child.children[0];
      child = { ...only, name: `${child.name}/${only.name}` };
    }
    node.children[idx] = child;
  }
}
