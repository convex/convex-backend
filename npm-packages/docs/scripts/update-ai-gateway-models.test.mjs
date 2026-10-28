import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { test } from "node:test";
import { pathToFileURL } from "node:url";

test("marks catalog matches and preserves the list when the catalog is invalid", () => {
  const dir = mkdtempSync(join(tmpdir(), "gateway-models-"));
  try {
    mkdirSync(join(dir, "scripts"));
    mkdirSync(join(dir, "docs/ai-gateway"), { recursive: true });
    const script = join(dir, "scripts/update.mjs");
    copyFileSync(
      new URL("./update-ai-gateway-models.mjs", import.meta.url),
      script,
    );
    const output = join(dir, "docs/ai-gateway/_models-list.mdx");
    for (const catalog of [
      { data: [{ model_id: "openai/matched" }] },
      { data: [] },
      { data: [{ model_id: 42 }] },
      {},
    ]) {
      writeFileSync(output, "previous catalog");
      const result = spawnSync(
        process.execPath,
        [
          "--input-type=module",
          "-e",
          `
        globalThis.fetch = async (url) => {
          if (url === "https://modest-hyena-982.convex.cloud/api/action") {
            return Response.json({ status: "success", value: ["openai/matched", "openai/unmatched"] });
          }
          if (url === "https://openrouter.ai/api/v1/endpoints/zdr") {
            return Response.json(${JSON.stringify(catalog)});
          }
          throw new Error("Unexpected fetch: " + url);
        };
        await import(${JSON.stringify(pathToFileURL(script).href)});
      `,
        ],
        {
          encoding: "utf8",
          env: { ...process.env, GITHUB_OUTPUT: join(dir, "github-output") },
        },
      );
      const markdown = readFileSync(output, "utf8");
      if (catalog.data?.[0]?.model_id === "openai/matched") {
        assert.equal(result.status, 0, result.stderr);
        assert.match(markdown, /^- `openai\/matched` · ZDR$/m);
        assert.match(markdown, /^- `openai\/unmatched`$/m);
      } else {
        assert.notEqual(result.status, 0);
        assert.match(
          result.stderr,
          /ZDR endpoint catalog is empty or has an unexpected shape/,
        );
        assert.equal(markdown, "previous catalog");
      }
    }
  } finally {
    rmSync(dir, { recursive: true, force: true });
  }
});
