// Origin used only to resolve `returnTo` for same-origin validation; never
// navigated to.
const VALIDATION_ORIGIN = "https://convex.invalid";

// Only honor a `returnTo` that resolves to a same-origin path.
export function safeReturnTo(value: unknown, fallback: string): string {
  if (typeof value !== "string" || !value.startsWith("/")) {
    return fallback;
  }
  // Disallow control characters (matching them is the intent here).
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001F\u007F]/.test(value)) {
    return fallback;
  }
  // Resolve exactly as a browser would (normalizing backslashes, etc.) and
  // require the result to stay on our own origin.
  try {
    if (new URL(value, VALIDATION_ORIGIN).origin !== VALIDATION_ORIGIN) {
      return fallback;
    }
  } catch {
    return fallback;
  }
  return value;
}
