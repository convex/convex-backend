import React, { useCallback, useContext, useRef } from "react";

// The rows rendered as links (see `ItemPrimary`), so ⌘/Ctrl+Enter can open the
// selected one in a new tab the way ⌘/Ctrl-clicking it does.
export type PaletteLinkRegistry = {
  // `onOpen` reports the selection's analytics; the registry opens the tab.
  register: (anchor: HTMLAnchorElement, onOpen: () => void) => () => void;
  // Opens the selected row in a new tab; null when the row isn't a link.
  newTabOpenerForSelection: () => (() => void) | null;
};

function selectedLink(): HTMLAnchorElement | null {
  return document.querySelector<HTMLAnchorElement>(
    '[cmdk-dialog] [cmdk-item][aria-selected="true"] > a[data-item-primary]',
  );
}

export function createPaletteLinkRegistry(): PaletteLinkRegistry {
  const links = new Map<HTMLAnchorElement, () => void>();
  return {
    register(anchor, onOpen) {
      links.set(anchor, onOpen);
      return () => {
        links.delete(anchor);
      };
    },
    newTabOpenerForSelection() {
      const anchor = selectedLink();
      if (!anchor) {
        return null;
      }
      return () => {
        // `anchor.href` is the URL next/link resolved, so the tab lands exactly
        // where ⌘-clicking the row would. `noopener` matches a ⌘-click too: the
        // new tab gets its own browsing context group instead of a handle back
        // to the dashboard. Pages can only open foreground tabs, so unlike a
        // ⌘-click, this one takes focus.
        window.open(anchor.href, "_blank", "noopener");
        links.get(anchor)?.();
      };
    },
  };
}

export const PaletteLinkContext =
  React.createContext<PaletteLinkRegistry | null>(null);

// A ref for a row's anchor that registers it with the palette.
export function useLinkRegistration(onOpen: (() => void) | undefined) {
  const registry = useContext(PaletteLinkContext);
  const latest = useRef(onOpen);
  latest.current = onOpen;
  return useCallback(
    (anchor: HTMLAnchorElement | null) =>
      anchor && registry
        ? registry.register(anchor, () => latest.current?.())
        : undefined,
    [registry],
  );
}
