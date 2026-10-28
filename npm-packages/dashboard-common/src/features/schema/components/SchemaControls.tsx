import { useReactFlow, useStore } from "@xyflow/react";
import * as Switch from "@radix-ui/react-switch";
import { PlusIcon, MinusIcon, ResetIcon } from "@radix-ui/react-icons";
import { ViewfinderCircleIcon } from "@heroicons/react/24/outline";
import { cn } from "@ui/cn";
import { Button } from "@ui/Button";
import { Tooltip } from "@ui/Tooltip";

export function SchemaControls({
  onResetLayout,
  clusteringEnabled,
  onToggleClustering,
}: {
  onResetLayout: () => void;
  clusteringEnabled: boolean;
  onToggleClustering: () => void;
}) {
  const { zoomIn, zoomOut, fitView } = useReactFlow();
  const zoom = useStore((s) => s.transform[2]);

  return (
    <div className="absolute bottom-3 left-3 z-10 flex items-center gap-1 rounded-lg border bg-background-secondary p-1 shadow-sm">
      <div className="flex items-center gap-0.5">
        <Tooltip tip="Zoom out" side="top" asChild>
          <Button
            size="xs"
            variant="neutral"
            inline
            className="border border-transparent"
            onClick={() => zoomOut()}
            icon={<MinusIcon />}
            aria-label="Zoom out"
          />
        </Tooltip>
        <span className="text-xs tabular-nums select-none">
          {Math.round(zoom * 100)}%
        </span>
        <Tooltip tip="Zoom in" side="top" asChild>
          <Button
            size="xs"
            variant="neutral"
            inline
            className="border border-transparent"
            onClick={() => zoomIn()}
            icon={<PlusIcon />}
            aria-label="Zoom in"
          />
        </Tooltip>
      </div>
      <div className="mx-0.5 h-5 border-l" />
      <Tooltip tip="Fit to view" side="top" asChild>
        <Button
          size="xs"
          variant="neutral"
          inline
          className="border border-transparent"
          onClick={() => fitView()}
          icon={<ViewfinderCircleIcon className="size-4" />}
          aria-label="Fit to view"
        />
      </Tooltip>
      <Tooltip tip="Reset layout" side="top" asChild>
        <Button
          size="xs"
          variant="neutral"
          inline
          className="border border-transparent"
          onClick={onResetLayout}
          icon={<ResetIcon />}
          aria-label="Reset layout"
        />
      </Tooltip>
      <div className="mx-0.5 h-5 border-l" />
      <label
        htmlFor="schema-clustering-toggle"
        title="Group related tables into clusters automatically"
        className="flex cursor-pointer items-center gap-1.5 rounded-sm px-1 py-0.5 text-xs select-none focus-within:ring-2 focus-within:ring-border-selected"
      >
        <Switch.Root
          id="schema-clustering-toggle"
          checked={clusteringEnabled}
          onCheckedChange={onToggleClustering}
          className={cn(
            "relative h-3 w-5 rounded-full transition-colors focus:outline-none",
            clusteringEnabled
              ? "bg-util-accent"
              : "bg-neutral-4 dark:bg-neutral-7",
          )}
        >
          <Switch.Thumb
            className={cn(
              "my-0.5 block size-2 rounded-full bg-white shadow-sm transition-transform",
              clusteringEnabled ? "translate-x-[10px]" : "translate-x-[2px]",
            )}
          />
        </Switch.Root>
        Automatic grouping
      </label>
    </div>
  );
}
