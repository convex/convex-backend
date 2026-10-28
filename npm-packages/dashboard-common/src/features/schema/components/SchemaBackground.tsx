import { Background, useStore } from "@xyflow/react";
import { zoomStep } from "@common/features/schema/lib/layout";

// <Background>'s dots scale with zoom, so zooming out collapses the grid into a
// smear. Snap the gap and dot size up by powers of two instead, so the grid just
// gets coarser in steps.
export const BASE_GAP = 24;
const BASE_DOT = 1;

export function AdaptiveBackground() {
  // Grid step depends on zoom alone; subscribe to zoom only.
  const factor = 2 ** useStore((s) => zoomStep(s.transform[2]));
  return (
    <Background
      gap={BASE_GAP * factor}
      size={BASE_DOT * factor}
      className="text-border-transparent"
    />
  );
}
