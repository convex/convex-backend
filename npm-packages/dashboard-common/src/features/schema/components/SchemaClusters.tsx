import { useCallback, useEffect, useRef } from "react";
import { useStoreApi } from "@xyflow/react";
import { Cluster } from "@common/features/schema/lib/clustering";
import { NODE_WIDTH, NodePositions } from "@common/features/schema/lib/layout";

// How far the hull extends past its tables' bounding box, in flow units. The
// top is larger to reserve a label band (matches `CLUSTER_TOP_PADDING` in
// elkLayout, which keeps that band clear of neighbouring clusters).
const HULL_PADDING = 24;
const HULL_TOP_PADDING = 56;
const HULL_RADIUS = 16;
// The label pill's height in screen px (a comfortable 24px click/drag target).
const PILL_HEIGHT = 24;
// Vertical gap left between labels when staggering overlapping ones.
const STAGGER_GAP = 4;
// Flow-unit nudge applied when arrow keys move a focused cluster (Shift = 5x).
const NUDGE_STEP = 10;
const NUDGE_STEP_LARGE = 50;

/**
 * Draws a translucent rounded "hull" behind each cluster's tables, so related
 * tables read as a group. The cluster's label is a themed DOM element rendered
 * by `SchemaClusterHandles` (over the reserved top band), not drawn here —
 * canvas text doesn't adapt to the theme and can't be edited.
 *
 * Uses the same pinned-canvas approach as `SchemaEdge` (and for the same
 * reasons): one canvas, never CSS-transformed, redrawn each frame through
 * `ctx.setTransform` from the live viewport transform and node positions. This
 * keeps the hulls glued to their tables as they pan, zoom, and drag, and avoids
 * the WebKit cost of dashed SVG strokes — the hulls are filled shapes.
 *
 * The hulls draw at z-index -1 (below the nodes, above the dot background),
 * before the edge layers in DOM order so edges paint on top of them.
 */
export function SchemaClusters({ clusters }: { clusters: Cluster[] }) {
  const storeApi = useStoreApi();
  const canvasRef = useRef<HTMLCanvasElement | null>(null);
  const clustersRef = useRef(clusters);
  clustersRef.current = clusters;

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

      // A single cluster would wrap the whole diagram — skip the hull (its
      // label pill still renders, from SchemaClusterHandles).
      if (clustersRef.current.length < 2) {
        return;
      }

      const [tx, ty, zoom] = transform;
      const style = getComputedStyle(canvas);
      // `color` is the accent (used for the border); `caretColor` carries a
      // neutral surface color for the fill — a paint-free property that still
      // resolves its CSS var, so the fill is a soft light-in-dark / dark-in-
      // light wash instead of an accent-tinted one.
      const strokeColor = style.color;
      const fillColor = style.caretColor;

      // Hulls in flow space, so they line up with the tables.
      ctx.setTransform(dpr * zoom, 0, 0, dpr * zoom, dpr * tx, dpr * ty);
      ctx.lineWidth = 1.5 / zoom; // ~1.5px on screen at any zoom.
      for (const cluster of clustersRef.current) {
        const box = clusterFlowBox(cluster, nodeLookup);
        if (!box) continue;
        traceRoundRect(
          ctx,
          box.x0 - HULL_PADDING,
          box.y0 - HULL_TOP_PADDING,
          box.x1 - box.x0 + HULL_PADDING * 2,
          box.y1 - box.y0 + HULL_TOP_PADDING + HULL_PADDING,
          HULL_RADIUS,
        );
        ctx.globalAlpha = 0.06;
        ctx.fillStyle = fillColor;
        ctx.fill();
        ctx.globalAlpha = 0.3;
        ctx.strokeStyle = strokeColor;
        ctx.stroke();
      }
      ctx.globalAlpha = 1;
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
    const unsubscribe = storeApi.subscribe(schedule);
    return () => {
      unsubscribe();
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [storeApi, clusters]);

  return (
    <canvas
      ref={canvasRef}
      className="absolute inset-0 size-full"
      style={{
        zIndex: -1,
        // Accent border via `color`; neutral fill via `caretColor` (paint-free,
        // read back resolved in the draw loop). See SchemaClusters draw().
        color: "var(--color-util-accent)",
        caretColor: "var(--color-content-primary)",
      }}
      aria-hidden
    />
  );
}

// A cluster's flow-space bounding box (over its tables' current positions),
// or null if none of its tables are placed yet.
type FlowBox = { x0: number; y0: number; x1: number; y1: number };

function clusterFlowBox(
  cluster: Cluster,
  nodeLookup: ReturnType<
    ReturnType<typeof useStoreApi>["getState"]
  >["nodeLookup"],
): FlowBox | null {
  let x0 = Infinity;
  let y0 = Infinity;
  let x1 = -Infinity;
  let y1 = -Infinity;
  let found = false;
  for (const table of cluster.tables) {
    const node = nodeLookup.get(table);
    if (!node) continue;
    found = true;
    const w = node.measured?.width ?? node.initialWidth ?? NODE_WIDTH;
    const h = node.measured?.height ?? node.initialHeight ?? 0;
    const { x, y } = node.internals.positionAbsolute;
    x0 = Math.min(x0, x);
    y0 = Math.min(y0, y);
    x1 = Math.max(x1, x + w);
    y1 = Math.max(y1, y + h);
  }
  return found ? { x0, y0, x1, y1 } : null;
}

/**
 * A draggable label pill for each cluster, positioned just above the cluster's
 * tables (in the band reserved by the hull's top padding, so it never overlaps
 * a node). Dragging a pill moves every table in that cluster together,
 * preserving their relative layout, then persists via `onDragStop`; arrow keys
 * nudge a focused pill.
 *
 * It's a themed DOM node rather than canvas text so it reads correctly in light
 * and dark mode. The pills are positioned imperatively each frame (rAF,
 * subscribed to the store) so they track their tables through pans, zooms, and
 * drags without re-rendering on every tick.
 */
export function SchemaClusterHandles({
  clusters,
  onMove,
  onDragStop,
}: {
  clusters: Cluster[];
  // Move the given tables to new absolute positions (called on each drag move).
  onMove: (positions: NodePositions) => void;
  onDragStop: () => void;
}) {
  const storeApi = useStoreApi();
  const pillRefs = useRef(new Map<string, HTMLElement | null>());
  const dragRef = useRef<{
    startClientX: number;
    startClientY: number;
    moved: boolean;
    zoom: number;
    tables: string[];
    base: NodePositions;
  } | null>(null);

  useEffect(() => {
    let frame = 0;
    const position = () => {
      const { transform, nodeLookup, width, height } = storeApi.getState();
      const [tx, ty, zoom] = transform;

      // Pass 1: compute each visible pill's desired screen position (and toggle
      // display) without reading layout, so we don't thrash reflow.
      const visible: { el: HTMLElement; left: number; top: number }[] = [];
      for (const cluster of clusters) {
        const el = pillRefs.current.get(cluster.id);
        if (!el) continue;
        const box = clusterFlowBox(cluster, nodeLookup);
        if (!box) {
          el.style.display = "none";
          continue;
        }
        // Hull rect in screen space.
        const hullLeft = (box.x0 - HULL_PADDING) * zoom + tx;
        const hullRight = (box.x1 + HULL_PADDING) * zoom + tx;
        const hullTop = (box.y0 - HULL_TOP_PADDING) * zoom + ty;
        const hullBottom = (box.y1 + HULL_PADDING) * zoom + ty;
        // Cull pills for clusters fully outside the viewport. This keeps the
        // focusable pills within the container's bounds, so Tab-focusing a node
        // (or a pill) can't scroll the overflow-hidden pane and knock the pinned
        // background/edge canvases out of alignment.
        if (
          width > 0 &&
          height > 0 &&
          (hullRight < 0 ||
            hullLeft > width ||
            hullBottom < 0 ||
            hullTop > height)
        ) {
          el.style.display = "none";
          continue;
        }
        // Anchor the pill just below the hull's top edge so it tracks the
        // group's outline as you zoom, but clamp it to stay above the topmost
        // table (the hull band shrinks with zoom; the pill is a fixed size).
        const nodeTop = box.y0 * zoom + ty;
        const left = hullLeft + 8;
        const top = Math.min(hullTop + 4, nodeTop - 4 - PILL_HEIGHT);
        el.style.display = "";
        visible.push({ el, left, top });
      }

      // Pass 2: stagger labels that would overlap (e.g. side-by-side clusters
      // whose labels collide). Measure widths in one batch, then greedily push
      // each colliding label straight down until it clears the ones placed
      // above it. Processing top-to-bottom keeps the topmost label in place.
      const widths = visible.map((v) => v.el.offsetWidth);
      const placed: { l: number; r: number; t: number; b: number }[] = [];
      visible
        .map((v, i) => ({ ...v, w: widths[i] }))
        .sort((a, b) => a.top - b.top || a.left - b.left)
        .forEach((v) => {
          let top = v.top;
          const right = v.left + v.w;
          for (let guard = 0; guard < 50; guard += 1) {
            const hit = placed.find(
              (p) =>
                v.left < p.r &&
                p.l < right &&
                top < p.b &&
                p.t < top + PILL_HEIGHT,
            );
            if (!hit) break;
            top = hit.b + STAGGER_GAP;
          }
          placed.push({ l: v.left, r: right, t: top, b: top + PILL_HEIGHT });
          v.el.style.transform = `translate(${v.left}px, ${top}px)`;
        });
    };
    const schedule = () => {
      if (!frame) {
        frame = window.requestAnimationFrame(() => {
          frame = 0;
          position();
        });
      }
    };
    schedule();
    const unsubscribe = storeApi.subscribe(schedule);
    return () => {
      unsubscribe();
      if (frame) window.cancelAnimationFrame(frame);
    };
  }, [storeApi, clusters]);

  // A pill sits on its own layer above the canvas, so a wheel over it wouldn't
  // reach React Flow's zoom handler. Re-dispatch the wheel to the pane so
  // scroll-to-zoom keeps working while hovering a label.
  const forwardWheel = useCallback(
    (e: React.WheelEvent) => {
      const pane = storeApi
        .getState()
        .domNode?.querySelector(".react-flow__pane");
      pane?.dispatchEvent(
        new WheelEvent("wheel", {
          deltaX: e.deltaX,
          deltaY: e.deltaY,
          deltaMode: e.deltaMode,
          clientX: e.clientX,
          clientY: e.clientY,
          bubbles: true,
          cancelable: true,
          ctrlKey: e.ctrlKey,
          metaKey: e.metaKey,
        }),
      );
    },
    [storeApi],
  );

  // Move all of a cluster's tables by a flow-space delta and persist — used by
  // the arrow-key nudge on a focused pill (mirrors moving a focused node).
  const nudgeCluster = useCallback(
    (tables: string[], dx: number, dy: number) => {
      const { nodeLookup } = storeApi.getState();
      const positions: NodePositions = {};
      tables.forEach((table) => {
        const node = nodeLookup.get(table);
        if (node) {
          positions[table] = {
            x: node.internals.positionAbsolute.x + dx,
            y: node.internals.positionAbsolute.y + dy,
          };
        }
      });
      onMove(positions);
      onDragStop();
    },
    [storeApi, onMove, onDragStop],
  );

  // A single cluster spans the whole diagram, so its label conveys nothing —
  // don't render it (matches the hull being skipped in SchemaClusters).
  if (clusters.length < 2) {
    return null;
  }

  return (
    <div
      // overflow-hidden so the pills (positioned with large transforms) never
      // extend the pane's scrollable area; the scroll reset undoes any
      // focus-induced scroll of this layer itself.
      className="pointer-events-none absolute inset-0 overflow-hidden"
      style={{ zIndex: 4 }}
      onScroll={(e) => {
        e.currentTarget.scrollTop = 0;
        e.currentTarget.scrollLeft = 0;
      }}
    >
      {clusters.map((cluster) => (
        // A raw <button>, not @ui/Button: it needs a forwarded ref for
        // imperative positioning and raw pointer handlers to own the drag.
        // eslint-disable-next-line react/forbid-elements
        <button
          key={cluster.id}
          type="button"
          ref={(el) => {
            pillRefs.current.set(cluster.id, el);
          }}
          // A mouse affordance, so keep it out of the tab sequence.
          tabIndex={-1}
          className="pointer-events-auto absolute top-0 left-0 flex h-6 min-w-6 origin-top-left cursor-grab items-center justify-center rounded-sm border border-border-transparent bg-background-secondary/90 px-2 text-xs font-medium whitespace-nowrap text-content-primary shadow-sm outline-none select-none hover:border-border-selected"
          aria-label={`Group “${cluster.label}”`}
          title="Drag to move"
          onWheel={forwardWheel}
          onKeyDown={(e) => {
            // Arrow keys move the whole group, mirroring how React Flow nudges
            // a focused node.
            const step = e.shiftKey ? NUDGE_STEP_LARGE : NUDGE_STEP;
            let dx = 0;
            let dy = 0;
            if (e.key === "ArrowUp") dy = -step;
            else if (e.key === "ArrowDown") dy = step;
            else if (e.key === "ArrowLeft") dx = -step;
            else if (e.key === "ArrowRight") dx = step;
            else return;
            e.preventDefault();
            e.stopPropagation();
            nudgeCluster(cluster.tables, dx, dy);
          }}
          onPointerDown={(e) => {
            // Take the drag ourselves instead of letting React Flow pan.
            e.stopPropagation();
            e.currentTarget.setPointerCapture(e.pointerId);
            e.currentTarget.style.cursor = "grabbing";
            const { transform, nodeLookup } = storeApi.getState();
            const base: NodePositions = {};
            cluster.tables.forEach((table) => {
              const node = nodeLookup.get(table);
              if (node) {
                base[table] = {
                  x: node.internals.positionAbsolute.x,
                  y: node.internals.positionAbsolute.y,
                };
              }
            });
            dragRef.current = {
              startClientX: e.clientX,
              startClientY: e.clientY,
              moved: false,
              zoom: transform[2],
              tables: cluster.tables,
              base,
            };
          }}
          onPointerMove={(e) => {
            const drag = dragRef.current;
            if (!drag) return;
            const dx = (e.clientX - drag.startClientX) / drag.zoom;
            const dy = (e.clientY - drag.startClientY) / drag.zoom;
            if (Math.abs(dx) > 1 || Math.abs(dy) > 1) {
              drag.moved = true;
            }
            const positions: NodePositions = {};
            drag.tables.forEach((table) => {
              const from = drag.base[table];
              if (from) {
                positions[table] = { x: from.x + dx, y: from.y + dy };
              }
            });
            onMove(positions);
          }}
          onPointerUp={(e) => {
            e.currentTarget.style.cursor = "grab";
            const drag = dragRef.current;
            dragRef.current = null;
            // Only persist when the cluster actually moved (a bare click or a
            // double-click to rename shouldn't rewrite the saved layout).
            if (drag?.moved) {
              onDragStop();
            }
          }}
        >
          {cluster.label}
        </button>
      ))}
    </div>
  );
}

// Trace a rounded rectangle path (starts a new path). Uses the native
// `roundRect` where available, falling back to arcs otherwise.
function traceRoundRect(
  ctx: CanvasRenderingContext2D,
  x: number,
  y: number,
  w: number,
  h: number,
  r: number,
) {
  ctx.beginPath();
  if (typeof ctx.roundRect === "function") {
    ctx.roundRect(x, y, w, h, r);
    return;
  }
  const radius = Math.min(r, w / 2, h / 2);
  ctx.moveTo(x + radius, y);
  ctx.arcTo(x + w, y, x + w, y + h, radius);
  ctx.arcTo(x + w, y + h, x, y + h, radius);
  ctx.arcTo(x, y + h, x, y, radius);
  ctx.arcTo(x, y, x + w, y, radius);
  ctx.closePath();
}
