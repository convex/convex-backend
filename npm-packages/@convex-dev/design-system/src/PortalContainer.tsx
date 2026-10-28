import { ReactNode, createContext, useContext } from "react";

const PortalContainerContext = createContext<HTMLElement | null>(null);

/**
 * Claims the portaled surfaces (combobox dropdowns, drawers) that descendants
 * open, mounting them in `container` instead of the document body. A popover
 * that dismisses itself when a click or focus lands outside its own element
 * points this at that element, so the surfaces opened from inside it are
 * inside it in the DOM too.
 */
export function PortalContainer({
  container,
  children,
}: {
  container: HTMLElement | null;
  children: ReactNode;
}) {
  return (
    <PortalContainerContext.Provider value={container}>
      {children}
    </PortalContainerContext.Provider>
  );
}

/**
 * The element to portal into, or null when nothing has claimed it and the
 * document body will do.
 */
export function usePortalContainer() {
  return useContext(PortalContainerContext);
}
