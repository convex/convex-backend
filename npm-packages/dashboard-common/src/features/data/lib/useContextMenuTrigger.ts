import {
  MouseEvent as ReactMouseEvent,
  RefObject,
  useCallback,
  useEffect,
  useRef,
} from "react";

// Returns React event handlers (rather than attaching native listeners to a
// ref) that open a context menu on right-click and close it when the right
// button is released after a long press. Prefer this for elements that are
// rendered in large numbers (e.g. table cells): React delegates events at the
// root, so it adds no per-element native listeners — and it works on the first
// right-click without depending on a prior hover/focus to attach a listener.
export function useContextMenuHandlers(
  onOpenContextMenu: (position: { x: number; y: number }) => void,
  onCloseContextMenu: () => void,
) {
  const allowMouseUpCloseRef = useRef(false);
  const timeoutRef = useRef<number | undefined>(undefined);

  const onContextMenu = useCallback(
    (e: ReactMouseEvent) => {
      e.preventDefault();
      onOpenContextMenu({ x: e.clientX, y: e.clientY });

      clearTimeout(timeoutRef.current);
      allowMouseUpCloseRef.current = false;
      timeoutRef.current = window.setTimeout(() => {
        allowMouseUpCloseRef.current = true;
      }, 300);
    },
    [onOpenContextMenu],
  );

  const onMouseUp = useCallback(() => {
    if (allowMouseUpCloseRef.current) {
      allowMouseUpCloseRef.current = false;
      onCloseContextMenu();
    }
  }, [onCloseContextMenu]);

  return { onContextMenu, onMouseUp };
}

// Based on https://codesandbox.io/s/trusting-rui-2duieo
export function useContextMenuTrigger(
  triggerRef: RefObject<HTMLElement | null>,
  onOpenContextMenu: (position: { x: number; y: number }) => void,
  onCloseContextMenu: () => void,
) {
  // When right-clicking for a long time, the context menu will disappear
  // when the right mouse button is released.
  const allowMouseUpCloseRef = useRef(false);

  useEffect(() => {
    if (!triggerRef || !triggerRef.current) return undefined;
    const trigger = triggerRef.current;

    let timeout: number;

    function onContextMenu(e: MouseEvent) {
      e.preventDefault();

      onOpenContextMenu({
        x: e.clientX,
        y: e.clientY,
      });

      clearTimeout(timeout);

      allowMouseUpCloseRef.current = false;
      timeout = window.setTimeout(() => {
        allowMouseUpCloseRef.current = true;
      }, 300);
    }

    function onMouseUp() {
      if (allowMouseUpCloseRef.current) {
        onCloseContextMenu();
      }
    }

    trigger.addEventListener("contextmenu", onContextMenu);
    trigger.addEventListener("mouseup", onMouseUp);
    return () => {
      trigger.removeEventListener("contextmenu", onContextMenu);
      trigger.removeEventListener("mouseup", onMouseUp);
      clearTimeout(timeout);
    };
  }, [triggerRef, onOpenContextMenu, onCloseContextMenu]);
}
