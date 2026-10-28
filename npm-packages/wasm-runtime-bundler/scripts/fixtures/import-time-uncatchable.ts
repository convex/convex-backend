// Throws an uncatchable developer error while evaluating, inside a `try` that
// must not see it.
declare const Convex: { op: (name: string, ...args: unknown[]) => unknown };

try {
  Convex.op("throwUncatchableDeveloperError", "thrown at import");
} catch {
  console.log("caught");
}
