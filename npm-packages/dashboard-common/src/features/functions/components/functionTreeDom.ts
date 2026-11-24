// `@pierre/trees` renders into a shadow root it owns and has no option for a
// search cursor, for opening rows in new tabs, or for turning off multiple
// selection. These helpers add them on top of the attributes it puts on its elements; `UNSAFE_CSS` in
// `FunctionTree` styles what they add.

const ROW_SELECTOR = "button[data-type='item']";
const FUNCTION_ROW_SELECTOR = "button[data-item-type='file'][data-item-path]";

export const SEARCH_CURSOR_ATTRIBUTE = "data-search-cursor";

export type SearchCursorMarker = {
  /** Re-reads `cursorPath` and moves the mark. */
  sync(): void;
  detach(): void;
};

/**
 * Marks the row at `cursorPath()` with `SEARCH_CURSOR_ATTRIBUTE`, and keeps it
 * marked as the tree recycles row elements while it scrolls.
 */
export function markSearchCursor(
  shadowRoot: ShadowRoot,
  cursorPath: () => string | null,
): SearchCursorMarker {
  const sync = () => {
    const cursor = cursorPath();
    for (const row of shadowRoot.querySelectorAll<HTMLElement>(ROW_SELECTOR)) {
      const marked = row.dataset.itemPath === cursor;
      if (row.hasAttribute(SEARCH_CURSOR_ATTRIBUTE) !== marked) {
        row.toggleAttribute(SEARCH_CURSOR_ATTRIBUTE, marked);
      }
    }
  };
  // Filtered to `data-item-path`, so the marks themselves don't trigger it.
  const observer = new MutationObserver(sync);
  observer.observe(shadowRoot, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ["data-item-path"],
  });
  sync();
  return { sync, detach: () => observer.disconnect() };
}

/**
 * Opens a function row's `hrefFor` in a new tab on a Cmd- or Ctrl-click or a
 * middle click, as a link would. The rows are buttons, which browsers don't
 * open in new tabs.
 */
export function openFunctionRowsInNewTab(
  shadowRoot: ShadowRoot,
  hrefFor: (path: string) => string | undefined,
) {
  const hrefAt = (event: Event) => {
    const row =
      event.target instanceof Element
        ? event.target.closest<HTMLElement>(FUNCTION_ROW_SELECTOR)
        : null;
    const path = row?.dataset.itemPath;
    return path === undefined ? undefined : hrefFor(path);
  };
  const onClick = (event: Event) => {
    if (
      event instanceof MouseEvent &&
      event.button === 0 &&
      (event.metaKey || event.ctrlKey)
    ) {
      const href = hrefAt(event);
      if (href !== undefined) {
        window.open(href, "_blank", "noopener");
      }
    }
  };
  const onAuxClick = (event: Event) => {
    if (event instanceof MouseEvent && event.button === 1) {
      const href = hrefAt(event);
      if (href !== undefined) {
        event.preventDefault();
        window.open(href, "_blank", "noopener");
      }
    }
  };
  // A middle press otherwise starts autoscrolling on Windows and Linux.
  const onMouseDown = (event: Event) => {
    if (
      event instanceof MouseEvent &&
      event.button === 1 &&
      hrefAt(event) !== undefined
    ) {
      event.preventDefault();
    }
  };
  shadowRoot.addEventListener("click", onClick, true);
  shadowRoot.addEventListener("auxclick", onAuxClick, true);
  shadowRoot.addEventListener("mousedown", onMouseDown, true);
  return () => {
    shadowRoot.removeEventListener("click", onClick, true);
    shadowRoot.removeEventListener("auxclick", onAuxClick, true);
    shadowRoot.removeEventListener("mousedown", onMouseDown, true);
  };
}

/**
 * Stops the tree's Shift-, Cmd- and Ctrl-click, Shift+Arrow, Cmd/Ctrl+A and
 * Cmd/Ctrl+Space from selecting several rows: the selection is the one open
 * function.
 */
export function preventMultipleSelection(shadowRoot: ShadowRoot) {
  const onClick = (event: Event) => {
    if (!(event instanceof MouseEvent)) {
      return;
    }
    const onRow =
      event.target instanceof Element &&
      event.target.closest(ROW_SELECTOR) !== null;
    if (onRow && (event.shiftKey || event.metaKey || event.ctrlKey)) {
      event.preventDefault();
      event.stopPropagation();
    }
  };
  const onKeyDown = (event: Event) => {
    if (!(event instanceof KeyboardEvent)) {
      return;
    }
    const extendsSelection =
      event.shiftKey && (event.key === "ArrowUp" || event.key === "ArrowDown");
    const selectsMore =
      (event.metaKey || event.ctrlKey) &&
      (event.key.toLowerCase() === "a" || event.key === " ");
    if (extendsSelection || selectsMore) {
      event.preventDefault();
      event.stopPropagation();
    }
  };
  // In the capture phase, so they run before the rows' own listeners.
  shadowRoot.addEventListener("click", onClick, true);
  shadowRoot.addEventListener("keydown", onKeyDown, true);
  return () => {
    shadowRoot.removeEventListener("click", onClick, true);
    shadowRoot.removeEventListener("keydown", onKeyDown, true);
  };
}
