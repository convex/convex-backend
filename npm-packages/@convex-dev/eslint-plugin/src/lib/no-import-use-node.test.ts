import path from "path";
import fs from "fs";
import { afterAll } from "vitest";
import { RuleTester } from "@typescript-eslint/rule-tester";
import { noImportUseNode } from "./no-import-use-node.js";

const projectRoot = fs.mkdtempSync(
  path.join(import.meta.dirname, "..", "test", "fixtures", "use-node-"),
);
const convexDir = path.join(projectRoot, "convex");
fs.mkdirSync(path.join(convexDir, "_generated"), { recursive: true });

function writeModule(name: string, source: string) {
  fs.writeFileSync(path.join(convexDir, name), source, { encoding: "utf-8" });
}

writeModule("useNode.ts", '"use node";\nexport const a = 1;\n');
writeModule("plain.ts", "export const a = 1;\n");
writeModule(
  "strictThenNode.ts",
  '"use strict";\n// a comment\n"use node";\nexport const a = 1;\n',
);
writeModule("nodeAfterStatement.ts", 'export const a = 1;\n"use node";\n');

afterAll(() => {
  fs.rmSync(projectRoot, { recursive: true, force: true });
});

const inConvexDir = (name: string) => path.join(convexDir, name);

const ruleTester = new RuleTester();

ruleTester.run("import-wrong-runtime", noImportUseNode, {
  valid: [
    {
      name: 'a "use node" file importing a "use node" module',
      filename: inConvexDir("importer.ts"),
      code: '"use node";\nimport { a } from "./useNode";\nexport const b = a;\n',
    },
    {
      name: 'a "use node" directive after "use strict" and comments',
      filename: inConvexDir("importer.ts"),
      code: '"use strict";\n// a comment\n"use node";\nimport { a } from "./useNode";\nexport const b = a;\n',
    },
    {
      name: 'a "use node" file importing a module that is not "use node"',
      filename: inConvexDir("importer.ts"),
      code: '"use node";\nimport { a } from "./plain";\nexport const b = a;\n',
    },
    {
      name: 'a JavaScript runtime file importing a module that is not "use node"',
      filename: inConvexDir("importer.ts"),
      code: 'import { a } from "./plain";\nexport const b = a;\n',
    },
    {
      name: 'a "use node" file importing a module that is not "use node" by name',
      filename: inConvexDir("importer.ts"),
      code: '"use node";\nimport { a } from "./nodeAfterStatement";\nexport const b = a;\n',
    },
    {
      name: "a package import from a JavaScript runtime file",
      filename: inConvexDir("importer.ts"),
      code: 'import { a } from "some-package";\nexport const b = a;\n',
    },
    {
      name: "an import that doesn't resolve to a file",
      filename: inConvexDir("importer.ts"),
      code: 'import { a } from "./doesNotExist";\nexport const b = a;\n',
    },
    {
      name: "a generated file, which is not an entry point",
      filename: inConvexDir(path.join("_generated", "server.ts")),
      code: 'import { a } from "../useNode";\nexport const b = a;\n',
    },
  ],
  invalid: [
    {
      name: 'a JavaScript runtime file importing a "use node" module',
      filename: inConvexDir("importer.ts"),
      code: 'import { a } from "./useNode";\nexport const b = a;\n',
      errors: [{ messageId: "wrong runtime import" }],
    },
    {
      name: 'a "use node" string that is not part of the directive prologue',
      filename: inConvexDir("importer.ts"),
      code: 'export const c = 1;\n"use node";\nimport { a } from "./useNode";\nexport const b = a;\n',
      errors: [{ messageId: "wrong runtime import" }],
    },
  ],
});
