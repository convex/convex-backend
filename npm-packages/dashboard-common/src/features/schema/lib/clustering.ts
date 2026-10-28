import { SchemaGraph } from "@common/features/schema/lib/buildSchemaGraph";

/**
 * A group of tables the diagram can label and box together. Two sources feed
 * these groups (see `computeClusters`):
 *
 *  1. Connected components: tables that transitively reference each other but
 *     are disconnected from the rest of the schema form their own island.
 *  2. Community detection: within a large connected component (where the
 *     component itself is no longer a useful grouping), Louvain modularity
 *     maximization splits the tables into densely-linked sub-groups.
 */
export type Cluster = {
  // Stable across renders for a given graph (derived from the members), so
  // React keys and layout stay put when unrelated tables change.
  id: string;
  // A default display name: the group's most-connected table (its "nucleus").
  // The UI lets the user rename a cluster; renames are stored separately and
  // keyed by `id`.
  label: string;
  // Member table names, sorted.
  tables: string[];
};

export type ClusterOptions = {
  // Modularity resolution passed to Louvain: higher → more, smaller
  // communities; lower → fewer, larger ones. 1.0 is the standard default.
  resolution?: number;
  // A connected component with fewer tables than this is kept as a single
  // cluster rather than subdivided — small islands are already a good group,
  // and community detection on them just produces noise.
  minSizeToSubdivide?: number;
};

const DEFAULT_OPTIONS: Required<ClusterOptions> = {
  resolution: 1,
  minSizeToSubdivide: 8,
};

/**
 * Group a schema's tables for labelling. The reference edges are treated as
 * undirected and unweighted-per-pair (parallel references between the same two
 * tables collapse to weight 1, matching how the diagram draws one relationship
 * line). Isolated tables (no references either way) are left ungrouped and
 * simply don't appear in any returned cluster.
 *
 * The result is deterministic for a given graph: tables are processed in the
 * order they appear in `graph.nodes` (the caller sorts them alphabetically),
 * and the community detection uses no randomness.
 */
export function computeClusters(
  graph: SchemaGraph,
  options: ClusterOptions = {},
): Cluster[] {
  const { resolution, minSizeToSubdivide } = { ...DEFAULT_OPTIONS, ...options };

  const n = graph.nodes.length;
  if (n === 0) {
    return [];
  }

  const indexOf = new Map<string, number>();
  graph.nodes.forEach((node, i) => indexOf.set(node.table, i));

  // Undirected weighted adjacency. A reference in either direction links the
  // pair; multiple references between the same pair sum but we add 1 each, so
  // the weight is "number of relationship lines between these two tables".
  const adjacency: Map<number, number>[] = Array.from(
    { length: n },
    () => new Map<number, number>(),
  );
  const link = (a: number, b: number) => {
    if (a === b) {
      return; // self-references don't group a table with anything.
    }
    adjacency[a].set(b, (adjacency[a].get(b) ?? 0) + 1);
    adjacency[b].set(a, (adjacency[b].get(a) ?? 0) + 1);
  };
  graph.edges.forEach((edge) => {
    const a = indexOf.get(edge.source);
    const b = indexOf.get(edge.target);
    if (a !== undefined && b !== undefined) {
      link(a, b);
    }
  });

  const components = connectedComponents(n, adjacency);

  // Each group is a list of member node indices.
  const groups: number[][] = [];
  components.forEach((members) => {
    // A lone table (or one whose only reference is to itself) isn't a group.
    if (members.length < 2) {
      return;
    }
    if (members.length < minSizeToSubdivide) {
      groups.push(members);
      return;
    }
    // Large component: split into communities, one group each.
    detectCommunities(members, adjacency, resolution).forEach((group) =>
      groups.push(group),
    );
  });

  // Default label is the group's "nucleus": the table most connected to the
  // rest of the group. The UI lets the user rename; the id is derived from the
  // members (not the label) so a rename survives unrelated schema changes.
  return groups.map((members) => {
    const tables = members.map((m) => graph.nodes[m].table).sort();
    return {
      id: `cluster:${tables[0]}`,
      label: graph.nodes[nucleus(members, adjacency)].table,
      tables,
    };
  });
}

// The most-connected member of a group: the node with the greatest total edge
// weight to other members. Ties break toward the lower index (alphabetically
// first, since node indices follow the sorted table order).
function nucleus(members: number[], adjacency: Map<number, number>[]): number {
  const memberSet = new Set(members);
  let best = members[0];
  let bestDegree = -1;
  members.forEach((m) => {
    let degree = 0;
    adjacency[m].forEach((weight, other) => {
      if (memberSet.has(other)) {
        degree += weight;
      }
    });
    if (degree > bestDegree) {
      bestDegree = degree;
      best = m;
    }
  });
  return best;
}

// --- Connected components (union-find) --------------------------------------

function connectedComponents(
  n: number,
  adjacency: Map<number, number>[],
): number[][] {
  const parent = Array.from({ length: n }, (_, i) => i);
  const find = (x: number): number => {
    let root = x;
    while (parent[root] !== root) {
      root = parent[root];
    }
    // Path compression.
    let cur = x;
    while (parent[cur] !== root) {
      const next = parent[cur];
      parent[cur] = root;
      cur = next;
    }
    return root;
  };
  const union = (a: number, b: number) => {
    const ra = find(a);
    const rb = find(b);
    if (ra !== rb) {
      parent[Math.max(ra, rb)] = Math.min(ra, rb);
    }
  };
  for (let i = 0; i < n; i++) {
    adjacency[i].forEach((_, j) => union(i, j));
  }

  const byRoot = new Map<number, number[]>();
  for (let i = 0; i < n; i++) {
    const root = find(i);
    const existing = byRoot.get(root);
    if (existing) {
      existing.push(i);
    } else {
      byRoot.set(root, [i]);
    }
  }
  // Members within a component keep ascending index order (i.e. alphabetical),
  // and components come out in ascending-root order — both deterministic.
  return Array.from(byRoot.keys())
    .sort((a, b) => a - b)
    .map((root) => byRoot.get(root)!);
}

// --- Community detection (Louvain modularity maximization) ------------------
//
// A compact, deterministic multi-level Louvain. Operates on the sub-adjacency
// induced by a component's members, returns the members partitioned into
// communities (each a list of the original node indices).

function detectCommunities(
  members: number[],
  adjacency: Map<number, number>[],
  resolution: number,
): number[][] {
  const size = members.length;
  const localOf = new Map<number, number>();
  members.forEach((m, i) => localOf.set(m, i));

  // Edges within the component, in local indices, each undirected pair once.
  const edges: [number, number, number][] = [];
  members.forEach((m, i) => {
    adjacency[m].forEach((weight, other) => {
      const j = localOf.get(other);
      if (j !== undefined && i < j) {
        edges.push([i, j, weight]);
      }
    });
  });

  const community = louvain(size, edges, resolution);

  // Group local nodes by community id, preserving ascending order, then map
  // back to original indices.
  const groups = new Map<number, number[]>();
  for (let i = 0; i < size; i++) {
    const c = community[i];
    const existing = groups.get(c);
    if (existing) {
      existing.push(members[i]);
    } else {
      groups.set(c, [members[i]]);
    }
  }
  return Array.from(groups.keys())
    .sort((a, b) => a - b)
    .map((c) => groups.get(c)!);
}

// Returns a community id (0..k-1) per node, from multi-level Louvain.
function louvain(
  n: number,
  edges: [number, number, number][],
  resolution: number,
): number[] {
  if (n === 0) {
    return [];
  }

  // Current (possibly aggregated) graph as adjacency maps. Aggregation can
  // introduce self-loops (edges internal to a merged community).
  let curN = n;
  let curAdj: Map<number, number>[] = Array.from(
    { length: n },
    () => new Map<number, number>(),
  );
  edges.forEach(([a, b, w]) => {
    curAdj[a].set(b, (curAdj[a].get(b) ?? 0) + w);
    curAdj[b].set(a, (curAdj[b].get(a) ?? 0) + w);
  });

  // Maps each original node to its node in the current aggregated graph.
  const nodeToSuper = Array.from({ length: n }, (_, i) => i);

  for (;;) {
    const { comm, improved } = louvainLevel(curN, curAdj, resolution);
    if (!improved) {
      break;
    }

    // Renumber the communities present to a contiguous 0..k-1, in
    // first-appearance order (deterministic).
    const superOf = new Map<number, number>();
    for (let i = 0; i < curN; i++) {
      if (!superOf.has(comm[i])) {
        superOf.set(comm[i], superOf.size);
      }
    }
    const k = superOf.size;

    for (let i = 0; i < n; i++) {
      nodeToSuper[i] = superOf.get(comm[nodeToSuper[i]])!;
    }
    if (k === curN) {
      break; // Nothing merged; no coarser level to build.
    }

    // Build the aggregated graph: one node per community, edge weights summed,
    // internal edges becoming self-loops.
    const newAdj: Map<number, number>[] = Array.from(
      { length: k },
      () => new Map<number, number>(),
    );
    const addWeight = (a: number, b: number, w: number) => {
      if (a === b) {
        newAdj[a].set(a, (newAdj[a].get(a) ?? 0) + w);
      } else {
        newAdj[a].set(b, (newAdj[a].get(b) ?? 0) + w);
        newAdj[b].set(a, (newAdj[b].get(a) ?? 0) + w);
      }
    };
    for (let i = 0; i < curN; i++) {
      const ai = superOf.get(comm[i])!;
      curAdj[i].forEach((w, j) => {
        if (j < i) {
          return; // Each undirected pair once; self-loop (j === i) counts once.
        }
        addWeight(ai, superOf.get(comm[j])!, w);
      });
    }

    curN = k;
    curAdj = newAdj;
  }

  return nodeToSuper;
}

// One level of local moving: repeatedly move each node into the neighbouring
// community that most increases modularity, until no move helps. Returns the
// per-node community assignment and whether anything moved.
function louvainLevel(
  n: number,
  adjacency: Map<number, number>[],
  resolution: number,
): { comm: number[]; improved: boolean } {
  // Weighted degree per node; a self-loop counts twice (standard convention).
  const degree = new Array<number>(n).fill(0);
  let twoM = 0;
  for (let i = 0; i < n; i++) {
    let deg = 0;
    adjacency[i].forEach((w, j) => {
      deg += w;
      if (j === i) {
        deg += w;
      }
    });
    degree[i] = deg;
    twoM += deg;
  }
  if (twoM === 0) {
    return { comm: Array.from({ length: n }, (_, i) => i), improved: false };
  }

  const comm = Array.from({ length: n }, (_, i) => i);
  const communityTotalDegree = degree.slice();

  let improved = false;
  let moved = true;
  while (moved) {
    moved = false;
    for (let i = 0; i < n; i++) {
      const from = comm[i];

      // Sum of edge weights from i into each neighbouring community (self-loops
      // excluded — they don't pull a node toward any other community).
      const toCommunity = new Map<number, number>();
      adjacency[i].forEach((w, j) => {
        if (j === i) {
          return;
        }
        const cj = comm[j];
        toCommunity.set(cj, (toCommunity.get(cj) ?? 0) + w);
      });

      // Tentatively remove i from its community.
      communityTotalDegree[from] -= degree[i];
      const ki = degree[i];

      // Baseline: staying put. gain = w(i, C) - resolution * Σtot_C * k_i / 2m.
      let bestComm = from;
      let bestGain =
        (toCommunity.get(from) ?? 0) -
        (resolution * communityTotalDegree[from] * ki) / twoM;

      toCommunity.forEach((w, c) => {
        if (c === from) {
          return;
        }
        const gain = w - (resolution * communityTotalDegree[c] * ki) / twoM;
        // Strictly-greater keeps ties with the current community, so the pass
        // is stable and order-deterministic.
        if (gain > bestGain) {
          bestGain = gain;
          bestComm = c;
        }
      });

      communityTotalDegree[bestComm] += ki;
      if (bestComm !== from) {
        comm[i] = bestComm;
        moved = true;
        improved = true;
      }
    }
  }

  return { comm, improved };
}
