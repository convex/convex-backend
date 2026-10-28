import { useEffect, useMemo, useRef, useState } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  Background,
  useReactFlow,
  useStore,
} from "@xyflow/react";
import { nodeSize } from "@common/features/schema/lib/layout";
import { nodeTypes } from "@common/features/schema/components/TableNode";
import { BASE_GAP } from "@common/features/schema/components/SchemaBackground";
import {
  SchemaEdge,
  CanvasEdge,
} from "@common/features/schema/components/SchemaEdge";
import { TableFlowNode } from "@common/features/schema/components/schemaFlowTypes";

// When zoomed out far enough that labels are unreadable, show a minimap-style
// inset (pinned top-right) of the area under the cursor at a readable zoom.
const PREVIEW_WIDTH = 320;
const PREVIEW_HEIGHT = 240;
const PREVIEW_ZOOM = 0.9;
// Below this main zoom, text is small enough to warrant the preview.
const PREVIEW_THRESHOLD = 0.6;
const PREVIEW_MARGIN = 12;

const noop = () => {};

type FlowPoint = { x: number; y: number };
// sx/sy: cursor relative to the canvas top-left; fx/fy: its flow-space position;
// cw: canvas width (to locate the right-pinned inset and detect cursor overlap).
type Pointer = { sx: number; sy: number; fx: number; fy: number; cw: number };

function hasNodeInView(nodes: TableFlowNode[], focus: FlowPoint): boolean {
  const halfWidth = PREVIEW_WIDTH / 2 / PREVIEW_ZOOM;
  const halfHeight = PREVIEW_HEIGHT / 2 / PREVIEW_ZOOM;
  const minX = focus.x - halfWidth;
  const maxX = focus.x + halfWidth;
  const minY = focus.y - halfHeight;
  const maxY = focus.y + halfHeight;
  return nodes.some((n) => {
    const { width, height } = nodeSize(
      n.data.node.fields.length,
      n.data.node.indexes.length,
    );
    return (
      n.position.x < maxX &&
      n.position.x + width > minX &&
      n.position.y < maxY &&
      n.position.y + height > minY
    );
  });
}

function ZoomPreviewCanvas({
  nodes,
  inactiveEdges,
  activeEdges,
  focus,
}: {
  nodes: TableFlowNode[];
  inactiveEdges: CanvasEdge[];
  activeEdges: CanvasEdge[];
  focus: FlowPoint;
}) {
  const { setViewport } = useReactFlow();

  useEffect(() => {
    void setViewport({
      x: PREVIEW_WIDTH / 2 - focus.x * PREVIEW_ZOOM,
      y: PREVIEW_HEIGHT / 2 - focus.y * PREVIEW_ZOOM,
      zoom: PREVIEW_ZOOM,
    });
  }, [setViewport, focus.x, focus.y]);

  // Memoize on the graph inputs so the inner React Flow subtree doesn't
  // reconcile on every cursor-move/pan frame (focus is applied via setViewport
  // above).
  return useMemo(
    () => (
      <ReactFlow
        nodes={nodes}
        nodeTypes={nodeTypes}
        onNodesChange={noop}
        defaultViewport={{ x: 0, y: 0, zoom: PREVIEW_ZOOM }}
        minZoom={PREVIEW_ZOOM}
        maxZoom={PREVIEW_ZOOM}
        nodesDraggable={false}
        nodesConnectable={false}
        elementsSelectable={false}
        panOnDrag={false}
        panOnScroll={false}
        zoomOnScroll={false}
        zoomOnPinch={false}
        zoomOnDoubleClick={false}
        preventScrolling={false}
        // The inset shows a small window, so cull the rest of the schema.
        onlyRenderVisibleElements
        proOptions={{ hideAttribution: true }}
        className="bg-background-primary"
      >
        <Background gap={BASE_GAP} className="text-border-transparent" />
        <SchemaEdge edges={inactiveEdges} appearance="inactive" />
        <SchemaEdge edges={activeEdges} appearance="active" />
      </ReactFlow>
    ),
    [nodes, inactiveEdges, activeEdges],
  );
}

export function MinimapOverlay({
  nodes,
  inactiveEdges,
  activeEdges,
}: {
  nodes: TableFlowNode[];
  inactiveEdges: CanvasEdge[];
  activeEdges: CanvasEdge[];
}) {
  // Subscribe to zoom only; the inset depends on the viewport only via `active`.
  const zoom = useStore((s) => s.transform[2]);
  const domNode = useStore((s) => s.domNode);
  const { screenToFlowPosition } = useReactFlow();

  // Keep the latest converter without re-attaching the listener each render.
  const screenToFlowRef = useRef(screenToFlowPosition);
  screenToFlowRef.current = screenToFlowPosition;

  const [pointer, setPointer] = useState<Pointer | null>(null);
  const rafRef = useRef(0);
  const pendingRef = useRef<MouseEvent | null>(null);

  const active = zoom < PREVIEW_THRESHOLD;

  // Track the cursor (throttled per frame) in screen coords (to place the inset)
  // and flow coords (to aim its contents).
  useEffect(() => {
    if (!active || !domNode) {
      return undefined;
    }
    const flush = () => {
      rafRef.current = 0;
      const e = pendingRef.current;
      if (!e) return;
      const rect = domNode.getBoundingClientRect();
      const flow = screenToFlowRef.current({ x: e.clientX, y: e.clientY });
      setPointer({
        sx: e.clientX - rect.left,
        sy: e.clientY - rect.top,
        fx: flow.x,
        fy: flow.y,
        cw: rect.width,
      });
    };
    const onMove = (e: MouseEvent) => {
      pendingRef.current = e;
      if (!rafRef.current) {
        rafRef.current = window.requestAnimationFrame(flush);
      }
    };
    const onLeave = () => {
      if (rafRef.current) {
        window.cancelAnimationFrame(rafRef.current);
        rafRef.current = 0;
      }
      setPointer(null);
    };
    domNode.addEventListener("mousemove", onMove);
    domNode.addEventListener("mouseleave", onLeave);
    return () => {
      domNode.removeEventListener("mousemove", onMove);
      domNode.removeEventListener("mouseleave", onLeave);
      if (rafRef.current) {
        window.cancelAnimationFrame(rafRef.current);
        rafRef.current = 0;
      }
    };
  }, [active, domNode]);

  // Only mount once the cursor is over the canvas: nothing to show before the
  // first move, and this keeps the second React Flow off the initial render path
  // (a fit-to-view below the threshold would otherwise render the schema twice).
  if (!active || !pointer) {
    return null;
  }

  const focus: FlowPoint = { x: pointer.fx, y: pointer.fy };

  // Hide the inset while the cursor is over it: it can't preview the area beneath
  // itself. Pointer-events-none means moves still reach the canvas, so it returns
  // once the cursor leaves the region.
  const overlapsInset =
    pointer.sx >= pointer.cw - PREVIEW_MARGIN - PREVIEW_WIDTH &&
    pointer.sx <= pointer.cw - PREVIEW_MARGIN &&
    pointer.sy >= PREVIEW_MARGIN &&
    pointer.sy <= PREVIEW_MARGIN + PREVIEW_HEIGHT;
  if (overlapsInset) {
    return null;
  }

  // Don't show a blank minimap.
  if (!hasNodeInView(nodes, focus)) {
    return null;
  }

  return (
    <div
      className="pointer-events-none absolute top-3 right-3 z-10 overflow-hidden rounded-lg border bg-background-secondary shadow-sm"
      style={{ width: PREVIEW_WIDTH, height: PREVIEW_HEIGHT }}
    >
      <ReactFlowProvider>
        <ZoomPreviewCanvas
          nodes={nodes}
          inactiveEdges={inactiveEdges}
          activeEdges={activeEdges}
          focus={focus}
        />
      </ReactFlowProvider>
    </div>
  );
}
