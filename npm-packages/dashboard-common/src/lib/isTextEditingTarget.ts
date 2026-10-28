// Whether `el` is somewhere the user is typing text, so single-character global
// shortcuts ("/", "?") must not steal the keystroke.
export function isTextEditingTarget(el: Element | null): boolean {
  if (!(el instanceof HTMLElement)) {
    return false;
  }
  if (
    el.tagName === "INPUT" ||
    el.tagName === "TEXTAREA" ||
    el.isContentEditable
  ) {
    return true;
  }
  // Monaco takes keystrokes through an EditContext-backed `div[role="textbox"]`
  // (`div.native-edit-context`) rather than a hidden textarea, so the checks
  // above miss every code/object editor in the dashboard.
  return el.closest('[role="textbox"], .monaco-editor') !== null;
}
