import { createRequire } from "module";

// Both `src/version.ts` and the bundled `dist/cli.js` sit one directory below
// the package root.
const pkg = createRequire(import.meta.url)("../package.json");

export const analyzerVersion: string = pkg.version;
