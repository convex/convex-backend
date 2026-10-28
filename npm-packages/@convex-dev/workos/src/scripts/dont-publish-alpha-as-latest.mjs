#!/usr/bin/env node

import { fileURLToPath } from "node:url";
import { dirname, join } from "node:path";
import { readFileSync } from "node:fs";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);

const packageJsonPath = join(__dirname, "../../package.json");
const packageJson = JSON.parse(readFileSync(packageJsonPath, "utf-8"));
const { version } = packageJson;

if (!(process.env.npm_config_user_agent ?? "").startsWith("pnpm")) {
  console.error("❌ Use `just pnpm publish` instead of `npm publish`");
  process.exit(1);
}

// Check if this is an alpha version
const isAlpha = version.includes("alpha");

if (isAlpha) {
  // For alpha versions, require --tag=alpha
  const npmTag = process.env.npm_config_tag;

  if (!npmTag || npmTag === "latest") {
    console.error(
      `❌ Alpha version ${version} cannot be published to "latest" tag`,
    );
    console.error("Use: just pnpm publish --tag=alpha");
    process.exit(1);
  }

  if (npmTag !== "alpha") {
    console.error(
      `❌ Alpha version ${version} should use --tag=alpha, not --tag=${npmTag}`,
    );
    console.error("Use: just pnpm publish --tag=alpha");
    process.exit(1);
  }

  console.log(`✅ Publishing alpha version ${version} with --tag=alpha`);
} else {
  // For stable versions, warn if not using latest
  const npmTag = process.env.npm_config_tag;
  if (npmTag && npmTag !== "latest") {
    console.warn(
      `⚠️  Publishing stable version ${version} with --tag=${npmTag}`,
    );
    console.warn("Did you mean to use --tag=latest or no tag?");
  } else {
    console.log(`✅ Publishing stable version ${version}`);
  }
}
