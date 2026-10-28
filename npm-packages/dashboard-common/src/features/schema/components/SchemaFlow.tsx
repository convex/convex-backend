import { useCallback, useEffect, useMemo, useRef, useState } from "react";
// Imported here (not via globals.css) because the relative @import there was
// silently dropped in production builds, hiding all nodes and the grid.
import "@xyflow/react/dist/style.css";
import {
  ReactFlow,
  ReactFlowProvider,
  useReactFlow,
  useNodesState,
  useStoreApi,
  useKeyPress,
  PanOnScrollMode,
  getNodesBounds,
  getViewportForBounds,
} from "@xyflow/react";
import { cn } from "@ui/cn";
import { SchemaGraph as SchemaGraphData } from "@common/features/schema/lib/buildSchemaGraph";
import {
  NodePositions,
  NodeSize,
  NODE_WIDTH,
  nodeSize,
  mergeSavedLayout,
} from "@common/features/schema/lib/layout";
import {
  SchemaSearch,
  buildSearchEntries,
} from "@common/features/schema/components/SchemaSearch";
import { useGlobalLocalStorage } from "@common/lib/useGlobalLocalStorage";
import { useMediaQuery } from "@common/lib/useMediaQuery";
import { computeElkLayout } from "@common/features/schema/lib/elkLayout";
import { computeClusters } from "@common/features/schema/lib/clustering";
import {
  nodeTypes,
  nodeAriaLabel,
} from "@common/features/schema/components/TableNode";
import {
  SchemaClusters,
  SchemaClusterHandles,
} from "@common/features/schema/components/SchemaClusters";
import { AdaptiveBackground } from "@common/features/schema/components/SchemaBackground";
import {
  SchemaEdge,
  CanvasEdge,
} from "@common/features/schema/components/SchemaEdge";
import { MinimapOverlay } from "@common/features/schema/components/SchemaMinimap";
import { SchemaControls } from "@common/features/schema/components/SchemaControls";
import {
  HoverTarget,
  TableFlowNode,
} from "@common/features/schema/components/schemaFlowTypes";

const VISIBLE_CULL_THRESHOLD = 80;

const MIN_ZOOM = 0.01;
const MAX_ZOOM = 2.5;
// Initial view fits the entire schema — allow zooming out all the way down to
// MIN_ZOOM so even a large graph fits in full rather than being clamped and
// cropped.
const INITIAL_FIT_VIEW = { padding: 0.15, minZoom: MIN_ZOOM };

// Stable empty set so unhighlighted nodes keep a constant data reference.
const NO_HIGHLIGHTS: ReadonlySet<string> = new Set();

function SchemaFlowInner({
  graph,
  storageKey,
  selectedTable,
  focusRequest,
  clustering = false,
  onSelectNode,
  onFocusTable,
  onClearSelection,
}: {
  graph: SchemaGraphData;
  storageKey: string;
  selectedTable: string | null;
  // Bump-on-each-request signal to pan a table into view (e.g. a side-panel
  // reference link). The nonce lets repeated requests for the same table re-fire.
  focusRequest: { table: string; nonce: number } | null;
  // When true, group related tables: lay each group out together and draw a
  // labelled hull behind it. Opt-in while we compare layouts on large graphs.
  clustering?: boolean;
  onSelectNode: (table: string, opts?: { fromKeyboard?: boolean }) => void;
  onFocusTable: (table: string) => void;
  onClearSelection: () => void;
}) {
  const { fitView, getNodes, setCenter, getViewport, setViewport } =
    useReactFlow();
  const storeApi = useStoreApi();

  const isTouch = useMediaQuery("(pointer: coarse)");
  // React Flow reads the pan direction per render rather than per wheel event,
  // so track Shift to switch the wheel to horizontal panning.
  const shiftPressed = useKeyPress("Shift");

  const sizes = useMemo<Record<string, NodeSize>>(() => {
    const result: Record<string, NodeSize> = {};
    graph.nodes.forEach((node) => {
      result[node.table] = nodeSize(node.fields.length, node.indexes.length);
    });
    return result;
  }, [graph]);

  // Whether automatic grouping is on. Persisted per deployment/component; the
  // `clustering` prop is only the initial default (so the toggle, once used,
  // sticks, and the comparison stories still honour their prop until toggled).
  const [clusteringEnabled, setClusteringEnabled] = useGlobalLocalStorage(
    `${storageKey}/clustering`,
    clustering,
  );

  // Related-table groups. Empty (and layout unchanged) unless clustering is on.
  const clusters = useMemo(
    () => (clusteringEnabled ? computeClusters(graph) : []),
    [graph, clusteringEnabled],
  );

  // Tables each table references (from edge source -> target), for node labels.
  const referencesByTable = useMemo<Map<string, string[]>>(() => {
    const map = new Map<string, string[]>();
    graph.edges.forEach((edge) => {
      const label =
        edge.source === edge.target ? `${edge.target} (itself)` : edge.target;
      const existing = map.get(edge.source);
      if (existing) {
        if (!existing.includes(label)) {
          existing.push(label);
        }
      } else {
        map.set(edge.source, [label]);
      }
    });
    return map;
  }, [graph.edges]);

  // Manual layout is saved per grouping mode: grouped and ungrouped arrange the
  // tables differently, so a custom arrangement in one mode must not overwrite
  // the other's. Ungrouped keeps the original key; grouped gets its own prefix.
  // Switching the toggle re-reads positions from the matching key.
  const [savedPositions, setSavedPositions, clearSavedPositions] =
    useGlobalLocalStorage<NodePositions>(
      clusteringEnabled ? `${storageKey}/elk-grouped` : `${storageKey}/elk`,
      {},
    );
  const savedPositionsRef = useRef(savedPositions);
  savedPositionsRef.current = savedPositions;

  const [nodes, setNodes, onNodesChange] = useNodesState<TableFlowNode>([]);
  const [hover, setHover] = useState<HoverTarget | null>(null);
  const [searchOpen, setSearchOpen] = useState(false);

  const searchEntries = useMemo(
    () => buildSearchEntries(graph, clusters),
    [graph, clusters],
  );

  // Frame a whole group in the viewport (picked from search): zoom/pan to fit
  // exactly its member tables.
  const fitCluster = useCallback(
    (tables: string[]) => {
      void fitView({
        nodes: tables.map((table) => ({ id: table })),
        padding: 0.3,
        // Cap the zoom so framing a small group doesn't blast in to 250%.
        maxZoom: 1,
        duration: 400,
      });
    },
    [fitView],
  );

  const pendingFocusRef = useRef<string | null>(null);
  const handledFocusNonceRef = useRef<number | null>(null);

  const buildNodes = useCallback(
    (positions: NodePositions): TableFlowNode[] =>
      graph.nodes.map((node) => {
        const size = sizes[node.table];
        return {
          id: node.table,
          type: "table",
          position: positions[node.table] ?? { x: 0, y: 0 },
          // Seed the estimated size so culling has bounds before React Flow
          // measures the node.
          initialWidth: size?.width,
          initialHeight: size?.height,
          // Spoken name for screen readers (read verbatim as the aria-label).
          ariaLabel: nodeAriaLabel(
            node,
            referencesByTable.get(node.table) ?? [],
          ),
          className: cn(
            "rounded-lg outline-none focus-visible:ring-2 focus-visible:ring-util-accent focus-visible:ring-offset-2 focus-visible:ring-offset-background-primary",
            // Promote each node to its own compositing layer. WebKit (unlike
            // Chrome) doesn't composite elements just for having an animated
            // 2D transform, so without this Safari repaints every node's
            // content on each pan/zoom frame, making large graphs very slow.
            "will-change-transform",
          ),
          data: {
            node,
            // Selection and highlights are overlaid at render by
            // `decoratedNodes`, so seed them empty here. Keeping selection out
            // of `buildNodes` means its identity only changes with the graph
            // (not on every table click), so the ELK effect below can depend on
            // it without re-running the layout on each selection.
            isSelected: false,
            highlightedFields: NO_HIGHLIGHTS,
            highlightedIndexes: NO_HIGHLIGHTS,
            onHover: setHover,
          },
        };
      }),
    [graph.nodes, sizes, referencesByTable],
  );

  // Place a new layout and fit the viewport to it in the same commit, so the
  // first painted frame is already fitted. Fitting a frame later (fitView
  // needs the nodes committed to the store first) would paint the whole graph
  // once at the stale viewport and then jump — an expensive wrong-layout flash
  // on large graphs, especially in Safari. The nodes' estimated sizes stand in
  // for measurements here; rows have fixed heights, so they agree.
  const placeAndFit = useCallback(
    (positions: NodePositions) => {
      const flowNodes = buildNodes(positions);
      // A focus-into-view request owns the viewport; don't fit over it.
      const fit = !pendingFocusRef.current;
      const { width, height } = storeApi.getState();
      if (fit && width && height) {
        // Move the viewport before committing the nodes so that the first
        // frame also culls against the fitted viewport (not just paints at
        // it).
        void setViewport(
          getViewportForBounds(
            getNodesBounds(flowNodes),
            width,
            height,
            INITIAL_FIT_VIEW.minZoom,
            MAX_ZOOM,
            INITIAL_FIT_VIEW.padding,
          ),
        );
      }
      setNodes(flowNodes);
      if (fit && (!width || !height)) {
        // The canvas hasn't been measured yet; fit once it has.
        window.requestAnimationFrame(() => {
          if (pendingFocusRef.current) return;
          void fitView(INITIAL_FIT_VIEW);
        });
      }
    },
    [buildNodes, setNodes, storeApi, setViewport, fitView],
  );

  // Re-run ELK whenever the graph (or clustering) changes, merging any saved
  // manual layout.
  useEffect(() => {
    let cancelled = false;
    void computeElkLayout(graph, sizes, clusters).then(({ positions }) => {
      if (cancelled) return;
      placeAndFit(
        mergeSavedLayout(savedPositionsRef.current, positions, sizes),
      );
    });
    return () => {
      cancelled = true;
    };
  }, [graph, sizes, clusters, placeAndFit]);

  const centerOnTable = useCallback(
    (table: string): boolean => {
      const node = getNodes().find((n) => n.id === table);
      if (!node) {
        return false;
      }
      const size = sizes[table];
      const centerX = node.position.x + (size?.width ?? NODE_WIDTH) / 2;
      const centerY = node.position.y + (size?.height ?? 0) / 2;
      void setCenter(centerX, centerY, {
        zoom: getViewport().zoom,
        duration: 400,
      });
      return true;
    },
    [getNodes, sizes, setCenter, getViewport],
  );

  useEffect(() => {
    if (!focusRequest || handledFocusNonceRef.current === focusRequest.nonce) {
      return;
    }
    // A table absent from the graph (e.g. a reference to a since-deleted table)
    // will never be placed. Mark the request handled and leave the pending ref
    // clear so the layout effect's fitView isn't blocked waiting for it.
    if (!graph.nodes.some((n) => n.table === focusRequest.table)) {
      handledFocusNonceRef.current = focusRequest.nonce;
      pendingFocusRef.current = null;
      return;
    }
    pendingFocusRef.current = focusRequest.table;
    if (!centerOnTable(focusRequest.table)) {
      // Node not placed yet; retry on the next `nodes` update.
      return;
    }
    handledFocusNonceRef.current = focusRequest.nonce;
    // Release the layout effect's fit-to-view a frame later: ELK's own
    // requestAnimationFrame(fitView) runs first and still sees the pending focus.
    window.requestAnimationFrame(() => {
      pendingFocusRef.current = null;
    });
  }, [focusRequest, nodes, centerOnTable, graph.nodes]);

  const highlights = useMemo(() => {
    const fields = new Map<string, Set<string>>();
    const indexes = new Map<string, Set<string>>();
    const addTo = (
      map: Map<string, Set<string>>,
      table: string,
      name: string,
    ) => {
      const existing = map.get(table);
      if (existing) {
        existing.add(name);
      } else {
        map.set(table, new Set([name]));
      }
    };
    if (hover?.kind === "field") {
      addTo(fields, hover.table, hover.field);
    } else if (hover?.kind === "index") {
      addTo(indexes, hover.table, hover.index);
    } else if (hover?.kind === "header") {
      graph.edges.forEach((edge) => {
        if (edge.target === hover.table) {
          addTo(fields, edge.source, edge.field);
        }
      });
    }
    if (selectedTable) {
      graph.edges.forEach((edge) => {
        if (edge.source === selectedTable) {
          addTo(fields, edge.source, edge.field);
        }
      });
    }
    return { fields, indexes };
  }, [graph.edges, hover, selectedTable]);

  // Selection and highlight state are pure derivations of `selectedTable` +
  // `highlights`, so overlay them onto the layout nodes at render rather than
  // writing them back into node state via an effect (which forced an extra
  // render on every hover/select). The identity short-circuit keeps unchanged
  // nodes referentially stable so React Flow skips re-rendering them.
  const decoratedNodes = useMemo<TableFlowNode[]>(
    () =>
      nodes.map((n) => {
        const isSelected = n.id === selectedTable;
        const highlightedFields = highlights.fields.get(n.id) ?? NO_HIGHLIGHTS;
        const highlightedIndexes =
          highlights.indexes.get(n.id) ?? NO_HIGHLIGHTS;
        if (
          n.data.isSelected === isSelected &&
          n.data.highlightedFields === highlightedFields &&
          n.data.highlightedIndexes === highlightedIndexes
        ) {
          return n;
        }
        return {
          ...n,
          data: {
            ...n.data,
            isSelected,
            highlightedFields,
            highlightedIndexes,
          },
        };
      }),
    [nodes, selectedTable, highlights],
  );

  // Edges draw on canvas layers, not SVG: SVG path costs scale with each
  // path's extent in both WebKit and Gecko — hundreds of long-range reference
  // edges freeze Firefox below 1fps, and dragging a selected hub node
  // (reshaping all of its edges every frame) ran at ~2.5fps in Safari as SVG.
  // Active (selected/hovered) edges go on their own layer, drawn above.
  const { inactiveEdges, activeEdges } = useMemo(() => {
    const inactive: CanvasEdge[] = [];
    const active: CanvasEdge[] = [];
    graph.edges.forEach((edge) => {
      const hoverActive =
        hover?.kind === "field"
          ? edge.source === hover.table && edge.field === hover.field
          : hover?.kind === "header"
            ? edge.target === hover.table
            : false;
      const isActive =
        hoverActive ||
        (selectedTable !== null &&
          (edge.source === selectedTable || edge.target === selectedTable));
      (isActive ? active : inactive).push({
        id: edge.id,
        source: edge.source,
        target: edge.target,
      });
    });
    return { inactiveEdges: inactive, activeEdges: active };
  }, [graph.edges, hover, selectedTable]);

  const persistPositions = useCallback(() => {
    const positions: NodePositions = {};
    getNodes().forEach((n) => {
      positions[n.id] = n.position;
    });
    setSavedPositions(positions);
  }, [getNodes, setSavedPositions]);

  // Move a set of tables to new positions in one commit (dragging a cluster).
  const moveClusterNodes = useCallback(
    (positions: NodePositions) => {
      setNodes((nds) =>
        nds.map((n) =>
          positions[n.id] ? { ...n, position: positions[n.id] } : n,
        ),
      );
    },
    [setNodes],
  );

  const resetLayout = useCallback(() => {
    clearSavedPositions();
    void computeElkLayout(graph, sizes, clusters).then(({ positions }) => {
      placeAndFit(positions);
    });
  }, [clearSavedPositions, graph, sizes, clusters, placeAndFit]);

  // Cull offscreen elements on large graphs only.
  const cullOffscreen =
    graph.nodes.length + graph.edges.length > VISIBLE_CULL_THRESHOLD;

  return (
    <div className="relative size-full">
      {/* Rendered before <ReactFlow> so Tab reaches the toggle ahead of the
          nodes (React Flow renders children after its tabIndex-0 nodes). */}
      <SchemaControls
        onResetLayout={resetLayout}
        clusteringEnabled={clusteringEnabled}
        onToggleClustering={() => setClusteringEnabled((prev) => !prev)}
      />
      <ReactFlow
        nodes={decoratedNodes}
        onlyRenderVisibleElements={cullOffscreen}
        nodeTypes={nodeTypes}
        onNodesChange={onNodesChange}
        onNodeClick={(_, node) => onSelectNode(node.id)}
        onNodeDragStop={persistPositions}
        // Only fires while focus is within the flow, so it doesn't hijack keys
        // elsewhere on the page.
        onKeyDown={(e) => {
          if (e.key === "Escape" && selectedTable) {
            onClearSelection();
            return;
          }
          // Open the focused table with Enter/Space, mirroring a click: React Flow
          // toggles its own selection on these keys but never fires onNodeClick.
          // Act only when the node wrapper itself is focused.
          if (e.key === "Enter" || e.key === " ") {
            const active = document.activeElement;
            if (
              active instanceof HTMLElement &&
              active.classList.contains("react-flow__node") &&
              active.dataset.id
            ) {
              onSelectNode(active.dataset.id, { fromKeyboard: true });
            }
          }
        }}
        panOnScroll
        panOnScrollMode={
          shiftPressed ? PanOnScrollMode.Horizontal : PanOnScrollMode.Free
        }
        minZoom={MIN_ZOOM}
        maxZoom={MAX_ZOOM}
        nodesDraggable={!isTouch}
        nodesConnectable={false}
        // Tab between tables; arrow keys move the focused one (React Flow built-in).
        nodesFocusable
        // Mark as an application region so a screen reader passes arrow keys
        // through to move the focused table rather than capturing them.
        aria-label="Database schema diagram. Press Tab to move between tables, then use the arrow keys to reposition the focused table and Enter to open its details."
        role="application"
        // Read-only viewer: never delete nodes via the keyboard.
        deleteKeyCode={null}
        proOptions={{ hideAttribution: true }}
        fitView
        fitViewOptions={INITIAL_FIT_VIEW}
        className="bg-background-primary"
      >
        <AdaptiveBackground />
        {/* After the background so they paint above the dots (all z-index -1,
            DOM order breaks the tie) and below the nodes; active above
            inactive. Cluster hulls go first so edges paint on top of them. */}
        {clusters.length > 0 && <SchemaClusters clusters={clusters} />}
        <SchemaEdge edges={inactiveEdges} appearance="inactive" />
        <SchemaEdge edges={activeEdges} appearance="active" />
        <SchemaSearch
          entries={searchEntries}
          onPick={onFocusTable}
          onPickCluster={fitCluster}
          onOpenChange={setSearchOpen}
        />
        {!searchOpen && (
          <MinimapOverlay
            nodes={decoratedNodes}
            inactiveEdges={inactiveEdges}
            activeEdges={activeEdges}
          />
        )}
        {clusters.length > 0 && !isTouch && (
          <SchemaClusterHandles
            clusters={clusters}
            onMove={moveClusterNodes}
            onDragStop={persistPositions}
          />
        )}
      </ReactFlow>
    </div>
  );
}

export function SchemaFlow(props: {
  graph: SchemaGraphData;
  storageKey: string;
  selectedTable: string | null;
  focusRequest: { table: string; nonce: number } | null;
  clustering?: boolean;
  onSelectNode: (table: string, opts?: { fromKeyboard?: boolean }) => void;
  onFocusTable: (table: string) => void;
  onClearSelection: () => void;
}) {
  return (
    <ReactFlowProvider>
      <SchemaFlowInner {...props} />
    </ReactFlowProvider>
  );
}
