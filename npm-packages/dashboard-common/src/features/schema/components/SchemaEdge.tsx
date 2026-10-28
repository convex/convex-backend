import { useEffect, useRef } from "react";
import { useStoreApi } from "@xyflow/react";
import { zoomStep, NODE_WIDTH } from "@common/features/schema/lib/layout";

export type CanvasEdge = { id: string; source: string; target: string };

// Inactive edges thin out to noise when zoomed out; drop their arrowheads at
// this zoom step (see `zoomStep`). Active edges keep theirs at any zoom.
const ARROWHEAD_HIDE_LEVEL = 2;

// Below this zoom we stop compensating edge weight for zoom. The compensation
// keeps a steady on-screen weight while zoomed in, but once the whole graph has
// shrunk to a speck a constant-weight edge overwhelms it — so past this point
// the cap lets edges thin out with everything else instead of staying fat.
const MIN_WEIGHT_COMPENSATION_ZOOM = 0.15;
const MAX_EDGE_LEVEL = zoomStep(MIN_WEIGHT_COMPENSATION_ZOOM);

// Stroke color and weight per appearance. Colors may be CSS color-mix()/var()
// expressions: they're set as the canvas element's `color`, so the browser
// resolves them against the current theme and each redraw reads the resolved
// value back once.
const APPEARANCE = {
  active: {
    color: "var(--color-util-accent)",
    strokeWidth: 2,
    arrowheads: "always",
  },
  inactive: {
    color:
      "color-mix(in srgb, var(--color-content-tertiary) 60%, var(--color-background-primary))",
    strokeWidth: 1.5,
    arrowheads: "zoom",
  },
} as const;

type Geometry = {
  // SVG path data (valid for a canvas `Path2D`).
  path: string;
  // Arrowhead tip and travel direction.
  tipX: number;
  tipY: number;
  dirX: number;
  dirY: number;
};

/**
 * Path and arrowhead placement for an edge from the source node's
 * bottom-center to the target node's top-center.
 */
function edgeGeometry(
  selfReference: boolean,
  sourceX: number,
  sourceY: number,
  targetX: number,
  targetY: number,
): Geometry {
  if (selfReference) {
    // Self-reference: one symmetric loop bulging off the node's right edge
    // (both endpoints sit at the node's horizontal center, so the right edge
    // is at sourceX + NODE_WIDTH / 2). Two control points out to the right,
    // mirrored above/below center, draw it as a single cubic. The opaque
    // arrowhead covers the line end.
    const edgeX = sourceX + NODE_WIDTH / 2;
    const cy = (sourceY + targetY) / 2;
    const gap = 7; // where the loop's two ends meet the edge, above/below center
    const outX = 40; // how far the loop reaches to the right
    const outY = 18; // half the loop's height
    const tipX = edgeX;
    const tipY = cy + gap;
    // Aim the arrowhead along the loop's incoming tangent (lower control
    // point → tip).
    const dx = tipX - (edgeX + outX);
    const dy = tipY - (cy + outY);
    const len = Math.hypot(dx, dy) || 1;
    return {
      path:
        `M ${edgeX},${cy - gap} ` +
        `C ${edgeX + outX},${cy - outY} ${edgeX + outX},${cy + outY} ${edgeX},${cy + gap}`,
      tipX,
      tipY,
      dirX: dx / len,
      dirY: dy / len,
    };
  }
  // Cubic bezier from the source's bottom into the target's top, matching
  // React Flow's getBezierPath (curvature 0.25). The trailing control point
  // sits directly above the target, so the curve always enters the top
  // vertically — the arrowhead points straight down.
  const curvature = 0.25;
  const span = targetY - sourceY;
  const offset = span >= 0 ? 0.5 * span : curvature * 25 * Math.sqrt(-span);
  const c1y = sourceY + offset;
  const c2y = targetY - offset;
  return {
    path: `M ${sourceX},${sourceY} C ${sourceX},${c1y} ${targetX},${c2y} ${targetX},${targetY}`,
    tipX: targetX,
    tipY: targetY,
    dirX: 0,
    dirY: 1,
  };
}

type CacheEntry = { key: string; geometry: Geometry; path: Path2D };

/**
 * Draws a set of edges into one canvas instead of one SVG path element each.
 *
 * SVG can't keep up with a large schema's edges: WebKit and Gecko both pay
 * per-path costs that scale with the path's extent, and long-range reference
 * edges span most of the graph — ~900 of them freeze Firefox below 1fps while
 * a canvas redraws them in a couple of milliseconds. The same applies to
 * dragging a selected hub node, where dozens of active SVG edges reshape per
 * frame (~2.5fps in Safari). The canvas element itself is never
 * CSS-transformed (that would make it one giant layer); it stays pinned to
 * the pane and each frame redraws through `ctx.setTransform` with the current
 * viewport transform, subscribed directly to the React Flow store so panning
 * doesn't go through React at all.
 *
 * Renders below the nodes and above the dot background via the same z-index
 * -1 + DOM-order trick React Flow's own `<Background>` uses; stack one
 * instance per appearance (inactive below, active above).
 */
export function SchemaEdge({
  edges,
  appearance,
}: {
  edges: CanvasEdge[];
  appearance: keyof typeof APPEARANCE;
}) {
  const storeApi = useStoreApi();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  // Path2D per edge, keyed by endpoint positions; entries rebuild when their
  // edge's endpoints move (drag/layout) and linger harmlessly for removed
  // edges until the graph remounts.
  const cacheRef = useRef(new Map<string, CacheEntry>());
  const edgesRef = useRef(edges);
  edgesRef.current = edges;

  useEffect(() => {
    let frame = 0;

    const draw = () => {
      const canvas = canvasRef.current;
      const ctx = canvas?.getContext("2d");
      if (!canvas || !ctx) return;
      const { transform, nodeLookup, width, height } = storeApi.getState();
      if (!width || !height) return;
      const dpr = window.devicePixelRatio || 1;
      const pixelWidth = Math.round(width * dpr);
      const pixelHeight = Math.round(height * dpr);
      if (canvas.width !== pixelWidth || canvas.height !== pixelHeight) {
        // Resizing also clears.
        canvas.width = pixelWidth;
        canvas.height = pixelHeight;
      } else {
        ctx.setTransform(1, 0, 0, 1, 0, 0);
        ctx.clearRect(0, 0, pixelWidth, pixelHeight);
      }

      const [x, y, zoom] = transform;
      // On-screen stroke weight is strokeWidth * zoom, so edges thin out when
      // zoomed out. Compensate with the quantized zoom factor to keep a steady
      // weight — but cap the compensation below MIN_WEIGHT_COMPENSATION_ZOOM so
      // edges thin out with the graph instead of dominating a zoomed-out speck.
      const level = Math.min(zoomStep(zoom), MAX_EDGE_LEVEL);
      const strokeWidth = APPEARANCE[appearance].strokeWidth * 2 ** level;
      const showArrowheads =
        APPEARANCE[appearance].arrowheads === "always" ||
        level < ARROWHEAD_HIDE_LEVEL;
      const resolved = getComputedStyle(canvas).color;

      ctx.setTransform(dpr * zoom, 0, 0, dpr * zoom, dpr * x, dpr * y);
      ctx.strokeStyle = resolved;
      ctx.fillStyle = resolved;
      ctx.lineWidth = strokeWidth;

      const cache = cacheRef.current;
      for (const edge of edgesRef.current) {
        const source = nodeLookup.get(edge.source);
        const target = nodeLookup.get(edge.target);
        if (!source || !target) continue;
        const sourceWidth =
          source.measured?.width ?? source.initialWidth ?? NODE_WIDTH;
        const sourceHeight =
          source.measured?.height ?? source.initialHeight ?? 0;
        const targetWidth =
          target.measured?.width ?? target.initialWidth ?? NODE_WIDTH;
        const sourceX = source.internals.positionAbsolute.x + sourceWidth / 2;
        const sourceY = source.internals.positionAbsolute.y + sourceHeight;
        const targetX = target.internals.positionAbsolute.x + targetWidth / 2;
        const targetY = target.internals.positionAbsolute.y;

        const key = `${sourceX},${sourceY},${targetX},${targetY}`;
        let entry = cache.get(edge.id);
        if (!entry || entry.key !== key) {
          const geometry = edgeGeometry(
            edge.source === edge.target,
            sourceX,
            sourceY,
            targetX,
            targetY,
          );
          entry = { key, geometry, path: new Path2D(geometry.path) };
          cache.set(edge.id, entry);
        }
        ctx.stroke(entry.path);

        if (showArrowheads) {
          const { tipX, tipY, dirX, dirY } = entry.geometry;
          const headLength = strokeWidth * 6;
          const headHalf = strokeWidth * 3;
          const baseX = tipX - dirX * headLength;
          const baseY = tipY - dirY * headLength;
          ctx.beginPath();
          ctx.moveTo(tipX, tipY);
          ctx.lineTo(baseX - dirY * headHalf, baseY + dirX * headHalf);
          ctx.lineTo(baseX + dirY * headHalf, baseY - dirX * headHalf);
          ctx.fill();
        }
      }
    };

    const schedule = () => {
      if (!frame) {
        frame = window.requestAnimationFrame(() => {
          frame = 0;
          draw();
        });
      }
    };

    schedule();
    // Redraw on any store change: viewport transform, node drags, resizes.
    // Coalesced to one draw per frame; a full 900-edge pass is ~1-2ms.
    const unsubscribe = storeApi.subscribe(schedule);
    return () => {
      unsubscribe();
      if (frame) window.cancelAnimationFrame(frame);
    };
    // `edges` is read through a ref, but redraw when it changes (hover moves
    // edges between the inactive and active layers).
  }, [storeApi, edges, appearance]);

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0 size-full"
      style={{ zIndex: -1, color: APPEARANCE[appearance].color }}
      aria-hidden
    />
  );
}
