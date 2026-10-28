import React from "react";

// State and actions the palette dialog's key handler needs, passed in so the
// handler stays a plain function that can be unit-tested without React.
export type PaletteKeyDownContext = {
  // Whether the palette is on a nested page (Escape/Backspace/ArrowLeft go back
  // instead of closing).
  inSubPage: boolean;
  // The current search input; Backspace only pops a page when it's empty.
  search: string;
  popPage: () => void;
  onClose: () => void;
  armDrillModifier: (active: boolean) => void;
  // A destructive action the active page has registered behind the
  // Cmd/Ctrl+Shift+Enter chord (e.g. bulk-deleting projects). Null when the
  // page has nothing to confirm.
  confirmAction: (() => void) | null;
  copySelection: () => boolean;
};

function hasSelectedInputText(event: React.KeyboardEvent): boolean {
  const target = event.target as HTMLInputElement;
  return (
    target.tagName === "INPUT" && target.selectionStart !== target.selectionEnd
  );
}

// Handles the palette dialog's keydown. Split out from CommandPaletteDialog so
// the (fiddly, event-dispatching) key logic can be tested directly.
export function handlePaletteKeyDown(
  event: React.KeyboardEvent,
  {
    inSubPage,
    search,
    popPage,
    onClose,
    armDrillModifier,
    confirmAction,
    copySelection,
  }: PaletteKeyDownContext,
) {
  if (
    event.key === "Enter" &&
    (event.metaKey || event.ctrlKey) &&
    event.shiftKey
  ) {
    // Cmd/Ctrl+Shift+Enter confirms the active page's destructive action. The
    // chord is intentionally awkward so it can't be triggered by accident.
    // Always swallow it (preventDefault also stops cmdk from selecting the
    // focused item, since cmdk skips its own Enter handling when the event's
    // default is already prevented) so it never toggles a row instead.
    event.preventDefault();
    event.stopPropagation();
    confirmAction?.();
  } else if (
    (event.key === "c" || event.key === "C") &&
    (event.metaKey || event.ctrlKey) &&
    !event.shiftKey &&
    !event.altKey
  ) {
    if (!hasSelectedInputText(event) && copySelection()) {
      event.preventDefault();
      event.stopPropagation();
    }
  } else if (event.key === "Escape") {
    event.preventDefault();
    if (inSubPage) {
      popPage();
    } else {
      onClose();
    }
  } else if (event.key === "Backspace" && !search && inSubPage) {
    event.preventDefault();
    popPage();
  } else if (event.key === "Tab" && !event.ctrlKey && !event.metaKey) {
    // Tab/Shift+Tab move the selection like the arrow keys instead of
    // moving browser focus out of the palette. Re-dispatch as an arrow key
    // so cmdk's own handler picks it up. Dispatch from the search input (the
    // event target), not the cmdk root: the synthetic event still bubbles up
    // to cmdk, but it also escapes to `document`, and giving it the input as
    // its target lets global shortcut handlers (react-hotkeys-hook, etc.)
    // skip it as they would any keystroke typed into a text field.
    // Dispatching from the root gave it a <div> target, slipping past those
    // guards so a stray Arrow key triggered page-level navigation.
    event.preventDefault();
    event.stopPropagation();
    (event.target as HTMLElement).dispatchEvent(
      new KeyboardEvent("keydown", {
        key: event.shiftKey ? "ArrowUp" : "ArrowDown",
        bubbles: true,
      }),
    );
  } else if (event.key === "Enter" && event.nativeEvent.isTrusted) {
    // Shift+Enter drills into the selected item's nested view. (The
    // synthetic Enter dispatched for ArrowRight below is not trusted, so it
    // doesn't clobber the armed modifier.)
    armDrillModifier(event.shiftKey);
  } else if (event.key === "ArrowRight") {
    // ArrowRight drills in, but only once the text cursor is at the end of
    // the input so it doesn't fight with editing the search.
    const target = event.target as HTMLInputElement;
    const inInput = target.tagName === "INPUT";
    if (
      !inInput ||
      (target.selectionStart === target.value.length &&
        target.selectionEnd === target.value.length)
    ) {
      event.preventDefault();
      armDrillModifier(true);
      // Dispatch from the input, not the cmdk root, so the synthetic Enter
      // that escapes to `document` carries a text-field target and is ignored
      // by global shortcut handlers (see the Tab branch above).
      target.dispatchEvent(
        new KeyboardEvent("keydown", { key: "Enter", bubbles: true }),
      );
    }
  } else if (event.key === "ArrowLeft" && inSubPage) {
    // ArrowLeft goes back up a layer from a nested view, but only once the
    // text cursor is at the start of the input so it doesn't fight with
    // editing the search.
    const target = event.target as HTMLInputElement;
    const inInput = target.tagName === "INPUT";
    if (
      !inInput ||
      (target.selectionStart === 0 && target.selectionEnd === 0)
    ) {
      event.preventDefault();
      popPage();
    }
  } else if (event.key === "ArrowUp" || event.key === "ArrowDown") {
    // cmdk moves the selection from its own handler on this element (which
    // runs before propagation reaches ancestors); stop the event here so it
    // can't reach page-level keyboard handlers outside the palette. The
    // synthetic ArrowUp dispatched for Shift+Tab would otherwise bubble to
    // the header's deployment switcher — a floating-ui list-navigation
    // handler that jumps to its first item ("provision dev deployment") and
    // navigates to /development.
    event.stopPropagation();
  }
}
