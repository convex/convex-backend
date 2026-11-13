import { expect, test } from "vitest";
import fs from "fs";
import os from "os";
import path from "path";
import { spawnSync } from "child_process";
import { ANALYZE_SCRIPT, parseAnalysisOutput } from "./localAnalysis.js";

test("parseAnalysisOutput reads the marker line and ignores noise", () => {
  const out =
    "some module logging\n\n@@convex-analysis@@" +
    JSON.stringify({
      "a.js": [{ name: "f", udfType: "Query", visibility: { kind: "public" } }],
    }) +
    "\n";
  const parsed = parseAnalysisOutput(out)!;
  expect(parsed["a.js"].functions[0].name).toBe("f");
  expect(parseAnalysisOutput("nothing")).toBeNull();
  expect(parseAnalysisOutput("@@convex-analysis@@{oops")).toBeNull();
});

test("the analysis script extracts registered functions from modules", () => {
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "convex-la-test-"));
  try {
    fs.writeFileSync(path.join(dir, "package.json"), '{"type":"module"}');
    fs.writeFileSync(
      path.join(dir, "m.js"),
      `const mk = (flags) => { const f = () => {}; Object.assign(f, flags); f.exportArgs = () => '{"type":"object","value":{}}'; f.exportReturns = () => '{"type":"number"}'; return f; };
export const get = mk({ isQuery: true, isPublic: true });
export const secret = mk({ isMutation: true, isInternal: true });
export const notAFunction = 5;
export const plain = () => 1;
`,
    );
    fs.writeFileSync(path.join(dir, "bad.js"), "throw new Error('boom');");
    const r = spawnSync(
      process.execPath,
      [
        "--input-type=module",
        "-e",
        ANALYZE_SCRIPT,
        "--",
        dir,
        "m.js",
        "bad.js",
      ],
      { encoding: "utf8" },
    );
    expect(r.status).toBe(0);
    const parsed = parseAnalysisOutput(r.stdout)!;
    expect(Object.keys(parsed)).toEqual(["m.js"]);
    expect(
      parsed["m.js"].functions.map((f) => [
        f.name,
        f.visibility?.kind,
        f.udfType,
      ]),
    ).toEqual([
      ["get", "public", "Query"],
      ["secret", "internal", "Mutation"],
    ]);
    expect(parsed["m.js"].functions[0].returns).toBe('{"type":"number"}');
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});
