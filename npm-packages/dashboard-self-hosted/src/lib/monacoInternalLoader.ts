import { loader } from "@monaco-editor/react";
import * as monaco from "monaco-editor";

// Monaco loads its language services in web workers. Turbopack bundles each
// `new Worker(new URL(…))` into its own chunk, which is what
// monaco-editor-webpack-plugin used to emit for the webpack build.
window.MonacoEnvironment = {
  getWorker(_workerId, label) {
    switch (label) {
      case "json":
        return new Worker(
          new URL(
            "monaco-editor/esm/vs/language/json/json.worker.js",
            import.meta.url,
          ),
        );
      // TypeScript and JavaScript share a worker.
      case "typescript":
      case "javascript":
        return new Worker(
          new URL(
            "monaco-editor/esm/vs/language/typescript/ts.worker.js",
            import.meta.url,
          ),
        );
      default:
        return new Worker(
          new URL(
            "monaco-editor/esm/vs/editor/editor.worker.js",
            import.meta.url,
          ),
        );
    }
  },
};

loader.config({ monaco });

loader
  .init()
  .then((_monacoInstance) => {
    /* ... */
  })
  .catch(console.error);
