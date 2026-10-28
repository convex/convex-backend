#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import packageJson from "../../package.json" with { type: "json" };

const tag = packageJson.version.includes("alpha") ? "alpha" : "latest";
const publishToken = `${packageJson.version}:${tag}`;

if (process.argv[2] === "--check") {
  if (
    process.env.npm_command === "publish" &&
    process.env.CONVEX_SAFE_PUBLISH !== publishToken
  ) {
    process.stderr.write(
      "Use `just pnpm run publish-package` to publish this package.\n",
    );
    process.exit(1);
  }
} else {
  const pnpm = process.env.npm_execpath;
  if (!pnpm || !(process.env.npm_config_user_agent ?? "").startsWith("pnpm")) {
    process.stderr.write(
      "Use `just pnpm run publish-package` to publish this package.\n",
    );
    process.exit(1);
  }

  const result = spawnSync(
    process.execPath,
    [
      pnpm,
      "publish",
      ...process.argv.slice(2),
      "--tag",
      tag,
      "--access",
      "public",
    ],
    {
      env: { ...process.env, CONVEX_SAFE_PUBLISH: publishToken },
      stdio: "inherit",
    },
  );
  process.exit(result.status ?? 1);
}
