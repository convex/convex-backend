import {
  FloatingFocusManager,
  FloatingPortal,
  Placement,
  autoUpdate,
  flip,
  offset,
  shift,
  useDismiss,
  useFloating,
  useInteractions,
  useMergeRefs,
  useRole,
} from "@floating-ui/react";
import { ReactNode, useState } from "react";
import { PortalContainer } from "@ui/PortalContainer";
import { cn } from "@ui/cn";

// A popover whose open state is owned by the caller, so chips can be opened
// programmatically (right after they're added) and closing can trigger side
// effects (applying the draft).
export function FloatingPanel({
  open,
  onOpenChange,
  button,
  children,
  placement = "bottom-start",
  className,
  buttonClassName,
  label,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  button: ReactNode;
  children: ReactNode;
  placement?: Placement;
  className?: string;
  buttonClassName?: string;
  label: string;
}) {
  const { refs, floatingStyles, context } = useFloating({
    open,
    onOpenChange,
    placement,
    middleware: [offset(6), flip(), shift({ padding: 8 })],
    whileElementsMounted: autoUpdate,
    // Placing the panel with top/left rather than a transform keeps it from
    // becoming the containing block of the `position: fixed` surfaces that
    // portal into it, like the combobox's full-screen dropdown on mobile.
    transform: false,
  });
  const dismiss = useDismiss(context);
  const role = useRole(context, { role: "dialog" });
  const { getReferenceProps, getFloatingProps } = useInteractions([
    dismiss,
    role,
  ]);
  // The panel dismisses itself when a click or focus lands outside its
  // element, so the dropdowns its contents open have to portal into it rather
  // than the document body.
  const [panel, setPanel] = useState<HTMLElement | null>(null);
  const floatingRef = useMergeRefs([refs.setFloating, setPanel]);

  return (
    <>
      <div
        ref={refs.setReference}
        {...getReferenceProps()}
        className={cn("inline-flex min-w-0", buttonClassName)}
      >
        {button}
      </div>
      {open && (
        <FloatingPortal>
          <FloatingFocusManager
            context={context}
            modal={false}
            initialFocus={-1}
            returnFocus={false}
          >
            <div
              ref={floatingRef}
              style={floatingStyles}
              {...getFloatingProps()}
              aria-label={label}
              className={cn(
                "z-50 flex flex-col gap-2 rounded-lg border bg-background-secondary p-2 shadow-md",
                className,
              )}
            >
              <PortalContainer container={panel}>{children}</PortalContainer>
            </div>
          </FloatingFocusManager>
        </FloatingPortal>
      )}
    </>
  );
}
