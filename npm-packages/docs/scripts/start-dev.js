#!/usr/bin/env node

const { execSync } = require("child_process");
const { detect } = require("detect-port");

async function startDev() {
  console.log("📁 Cleaning docs/api directory...");
  execSync("npx rimraf docs/api", { stdio: "inherit" });

  console.log("🧹 Preparing OpenAPI specs (drop alpha, mark beta)...");
  execSync("node scripts/prepare-openapi-specs.js", { stdio: "inherit" });

  console.log("📚 Generating platform API documentation...");
  execSync("npm run generate-platform-api", { stdio: "inherit" });

  const defaultPort = Number(process.env.PORT) || 3000;
  const port = await detect(defaultPort);

  if (port !== defaultPort) {
    console.log(
      `⚠️  Port ${defaultPort} is in use, using port ${port} instead.\n`,
    );
  } else {
    console.log(`✅ Using port ${port}\n`);
  }

  console.log(`🌐 Starting Docusaurus server on port ${port}...`);
  console.log(
    `📖 Documentation will be available at: http://localhost:${port}\n`,
  );

  execSync(`docusaurus start --port ${port}`, { stdio: "inherit" });
}

startDev();
