// Reaches the database while evaluating, which no import-phase host serves.
declare const Convex: { syscall: (name: string, args: string) => string };

Convex.syscall("insert", JSON.stringify({ table: "t", value: {} }));
