import { type ElkNode } from "elkjs/lib/elk.bundled.js";
import { SchemaGraph } from "@common/features/schema/lib/buildSchemaGraph";
import { Cluster } from "@common/features/schema/lib/clustering";
import {
  NodePositions,
  NodeSize,
  NODE_WIDTH,
} from "@common/features/schema/lib/layout";

type ElkInstance = { layout: (graph: ElkNode) => Promise<ElkNode> };
let elkPromise: Promise<ElkInstance> | undefined;
function getElk(): Promise<ElkInstance> {
  if (!elkPromise) {
    elkPromise = import("elkjs/lib/elk.bundled.js").then(
      (mod) => new mod.default() as ElkInstance,
    );
  }
  return elkPromise;
}

const LAYOUT_OPTIONS = {
  "elk.algorithm": "layered",
  "elk.layered.nodePlacement.favorStraightEdges": "true",
  "elk.layered.nodePlacement.strategy": "BRANDES_KOEPF",
  "elk.direction": "DOWN",
  "elk.spacing.nodeNode": "64",
  "elk.spacing.edgeNode": "24",
  "elk.layered.spacing.nodeNodeBetweenLayers": "120",
  "elk.spacing.edgeEdge": "10",
  // Pack disconnected tables (each its own component) close together instead
  // of scattering them across the canvas.
  "elk.spacing.componentComponent": "64",
};

// Per-cluster layout: a nested container laid out on its own. The extra top
// padding reserves a band above the tables for the cluster's label, so the
// label never has to overlap a node. Keep this in sync with `HULL_TOP_PADDING`
// in SchemaClusters, which draws the label into that band.
const CLUSTER_TOP_PADDING = 56;
const CLUSTER_LAYOUT_OPTIONS = {
  "elk.algorithm": "layered",
  "elk.direction": "DOWN",
  "elk.padding": `[top=${CLUSTER_TOP_PADDING},left=24,bottom=24,right=24]`,
  "elk.spacing.nodeNode": "48",
  "elk.layered.spacing.nodeNodeBetweenLayers": "96",
};

export async function computeElkLayout(
  graph: SchemaGraph,
  sizes: Record<string, NodeSize>,
  // When provided, clustered tables are laid out inside a nested container per
  // cluster (so a group's tables stay physically together), and the containers
  // are packed at the top level. Tables in no cluster lay out at the top level.
  clusters: Cluster[] = [],
): Promise<{ positions: NodePositions }> {
  if (graph.nodes.length === 0) {
    return { positions: {} };
  }

  // ELK can't route a self-loop the same way; skip those (React Flow draws its
  // own self-loop edge).
  const edges = graph.edges
    .filter((edge) => edge.source !== edge.target)
    .map((edge) => ({
      id: edge.id,
      sources: [edge.source],
      targets: [edge.target],
    }));

  const nodeChild = (table: string): ElkNode => ({
    id: table,
    width: sizes[table]?.width ?? NODE_WIDTH,
    height: sizes[table]?.height ?? 100,
  });

  const elk = await getElk();

  if (clusters.length === 0) {
    const laidOut = await elk.layout({
      id: "root",
      layoutOptions: LAYOUT_OPTIONS,
      children: graph.nodes.map((node) => nodeChild(node.table)),
      edges,
    });
    const positions: NodePositions = {};
    (laidOut.children ?? []).forEach((child) => {
      positions[child.id] = { x: child.x ?? 0, y: child.y ?? 0 };
    });
    return { positions };
  }

  // Hierarchical layout: one container node per cluster, unclustered tables at
  // the root alongside them.
  const clusterOf = new Map<string, Cluster>();
  clusters.forEach((cluster) =>
    cluster.tables.forEach((table) => clusterOf.set(table, cluster)),
  );

  // Each cluster lays out with its own intra-cluster edges (so its internal
  // shape reflects its relationships). Cross-cluster edges are left out of ELK:
  // the top-level packing arranges the clusters purely by size for a balanced
  // aspect ratio, and those edges are drawn (as beziers) at render time anyway.
  const clusterEdges = new Map<string, typeof edges>();
  clusters.forEach((cluster) => clusterEdges.set(cluster.id, []));
  edges.forEach((edge) => {
    const source = clusterOf.get(edge.sources[0]);
    const target = clusterOf.get(edge.targets[0]);
    if (source && target && source.id === target.id) {
      clusterEdges.get(source.id)!.push(edge);
    }
  });

  const clusterContainers = new Map<string, ElkNode>();
  clusters.forEach((cluster) => {
    clusterContainers.set(cluster.id, {
      id: cluster.id,
      layoutOptions: CLUSTER_LAYOUT_OPTIONS,
      children: [],
      edges: clusterEdges.get(cluster.id),
    });
  });

  const rootChildren: ElkNode[] = [];
  graph.nodes.forEach((node) => {
    const cluster = clusterOf.get(node.table);
    if (cluster) {
      clusterContainers.get(cluster.id)!.children!.push(nodeChild(node.table));
    } else {
      rootChildren.push(nodeChild(node.table));
    }
  });
  clusters.forEach((cluster) =>
    rootChildren.push(clusterContainers.get(cluster.id)!),
  );

  // Pack the clusters (and any loose tables) into a balanced ~16:10 area rather
  // than the tall single column a layered top-level layout would produce.
  const laidOut = await elk.layout({
    id: "root",
    layoutOptions: {
      "elk.algorithm": "rectpacking",
      "elk.aspectRatio": "1.6",
      "elk.spacing.nodeNode": "48",
      "elk.padding": "[top=24,left=24,bottom=24,right=24]",
    },
    children: rootChildren,
  });

  // Flatten nested child positions to absolute coordinates (a cluster's tables
  // come back positioned relative to their container).
  const positions: NodePositions = {};
  (laidOut.children ?? []).forEach((child) => {
    if (child.children && child.children.length > 0) {
      const baseX = child.x ?? 0;
      const baseY = child.y ?? 0;
      child.children.forEach((grandchild) => {
        positions[grandchild.id] = {
          x: baseX + (grandchild.x ?? 0),
          y: baseY + (grandchild.y ?? 0),
        };
      });
    } else {
      positions[child.id] = { x: child.x ?? 0, y: child.y ?? 0 };
    }
  });

  return { positions };
}
