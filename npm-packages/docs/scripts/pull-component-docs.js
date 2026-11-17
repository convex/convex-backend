const fs = require("fs");
const path = require("path");
const { execSync } = require("child_process");

/**
 * Cross-platform script to pull agent docs and process them
 * Replaces Unix-specific find and sed commands
 */

async function processFiles(dirPath, repo) {
  const files = fs.readdirSync(dirPath);

  for (const file of files) {
    const filePath = path.join(dirPath, file);
    const stat = fs.statSync(filePath);

    if (stat.isDirectory()) {
      await processFiles(filePath); // Recursive
    } else if (file.endsWith(".mdx")) {
      // Process .mdx files - replace relative links with GitHub links
      let content = fs.readFileSync(filePath, "utf8");

      // Replace patterns like ](../ with ](https://github.com/${repo}/blob/main/
      // e.g. for agent: [somefile](../foo/bar.ts) -> [somefile](https://github.com/convex/agent/blob/main/foo/bar.ts)
      content = content.replace(
        /\]\(\.\.\//g,
        `](https://github.com/${repo}/blob/main/`,
      );

      fs.writeFileSync(filePath, content);
      console.log(`Processed: ${filePath}`);
    }
  }
}

const repos = [
  {
    repo: "convex/agent",
    fromDir: "docs",
    toDir: "agents",
  },
];

async function main() {
  for (const { repo, fromDir, toDir } of repos) {
    console.log(`Pulling ${repo} docs from GitHub...`);
    execSync(`npx degit github:${repo}/${fromDir} ./docs/${toDir} --force`, {
      stdio: "inherit",
    });

    console.log(`Processing .mdx files for ${repo}...`);
    await processFiles(`./docs/${toDir}`, repo);

    console.log(`Running prettier for ${repo}...`);
    execSync(`npx prettier -w ./docs/${toDir}`, { stdio: "inherit" });
  }
  console.log(`✅ Docs pulled and processed successfully!`);
  try {
    execSync(`npm run spellcheck`, { stdio: ["inherit", "inherit"] });
    console.log(`✅ Spellcheck passed!`);
  } catch {
    console.log(`❌ Spellcheck failed!`);
    process.exit(1);
  }
}

main();
