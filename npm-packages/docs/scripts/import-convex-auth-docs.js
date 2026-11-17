const crypto = require("crypto");
const fs = require("fs");
const os = require("os");
const path = require("path");
const { execFileSync, execSync } = require("child_process");

/**
 * Imports the Convex Auth docs of a release of
 * https://github.com/convex/convex-auth: the guides into
 * docs/auth/convex-auth, and the API reference into docs/convex-auth-api, which
 * sits in the sidebar with the other API references.
 *
 * Usage: node scripts/import-convex-auth-docs.js <tag>
 *
 * The `publish-docs` workflow of that repo attaches the docs of each release
 * tag to its GitHub Release, with the API reference generated and the code
 * blocks inlined. This script downloads that archive and copies its Markdown,
 * doing some light transformations along the way.
 */

const releaseUrl = "https://github.com/convex/convex-auth/releases/download";
const assetName = "convex-auth-docs.tar.gz";

const guidesDir = path.resolve(__dirname, "../docs/auth/convex-auth");
const apiDir = path.resolve(__dirname, "../docs/convex-auth-api");
// The directory of the archive that holds the generated API reference.
const srcApiDirName = "api";
// Written by the export next to the docs; it is not a page.
const manifestName = "manifest.json";
// Site-absolute links and doc ids of the source site are relative to its
// root; here the guides live under this prefix. The API reference links only
// relatively, within itself.
const guidesPrefix = "auth/convex-auth";
// YAML comment lines that open the front matter of every imported page, where
// it is the first thing a reader of the source sees.
const generatedComment = [
  "# NOTE: imported from https://github.com/convex/convex-auth by",
  "# scripts/import-convex-auth-docs.js. Edit the source in that repo and",
  "# re-import with `just import-convex-auth-docs <tag>`. If you need to make",
  "# changes here, make sure to upstream them to the source repo too or",
  "# they'll be overwritten on the next import.",
].join("\n");

function addGeneratedComment(content) {
  if (/^---\r?\n/.test(content)) {
    return content.replace(/^---\r?\n/, `---\n${generatedComment}\n`);
  }
  return `---\n${generatedComment}\n---\n\n${content}`;
}

function prefixAbsoluteLinks(content, prefix) {
  return (
    content
      // Markdown links: [text](/foo) -> [text](/auth/convex-auth/foo)
      .replace(/\]\(\/(?!\/)/g, `](/${prefix}/`)
      // JSX props and object literals, e.g. the items of <DocCardList>:
      // href="/foo", href: "/foo", docId: "foo"
      .replace(/\b(href|to)(=|:\s*)"\/(?!\/)/g, `$1$2"/${prefix}/`)
      .replace(
        new RegExp(`\\bdocId(=|:\\s*)"(?!${prefix}/)`, "g"),
        `docId$1"${prefix}/`,
      )
  );
}

function copyDocs(fromDir, toDir, options) {
  const { prefix, exclude = [] } = options;
  fs.mkdirSync(toDir, { recursive: true });
  for (const entry of fs.readdirSync(fromDir, { withFileTypes: true })) {
    if (exclude.includes(entry.name)) continue;
    const from = path.join(fromDir, entry.name);
    const to = path.join(toDir, entry.name);
    if (entry.isDirectory()) {
      copyDocs(from, to, { prefix });
    } else if (/\.mdx?$/.test(entry.name)) {
      let content = fs.readFileSync(from, "utf8");
      if (prefix) content = prefixAbsoluteLinks(content, prefix);
      fs.writeFileSync(to, addGeneratedComment(content));
    } else {
      fs.copyFileSync(from, to);
    }
  }
}

async function download(url) {
  const response = await fetch(url);
  if (!response.ok) {
    throw new Error(`GET ${url}: ${response.status} ${response.statusText}`);
  }
  return Buffer.from(await response.arrayBuffer());
}

/** Throws for a tar entry path that could resolve outside the extraction directory. */
function checkEntryPath(name) {
  if (
    name.startsWith("/") ||
    name.includes("\\") ||
    name.split("/").includes("..")
  ) {
    throw new Error(`unsafe path in the archive: ${JSON.stringify(name)}`);
  }
}

/**
 * Throws for anything in `dir` that is not a regular file or a directory,
 * counting a hardlink as a link.
 */
function checkOnlyFilesAndDirs(dir) {
  for (const entry of fs.readdirSync(dir, { withFileTypes: true })) {
    const p = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      checkOnlyFilesAndDirs(p);
    } else if (!entry.isFile() || fs.lstatSync(p).nlink > 1) {
      throw new Error(`the archive has ${p}, not a file or a directory`);
    }
  }
}

/**
 * Extracts a .tar.gz into `destDir`. GNU tar and bsdtar already refuse
 * absolute and `..` paths and writing through symlinks; the checks here make
 * an archive with any of those, or with a link, fail the import outright.
 */
function extractTarGz(archivePath, destDir) {
  const names = execFileSync("tar", ["-tzf", archivePath], {
    encoding: "utf8",
  });
  names.split("\n").filter(Boolean).forEach(checkEntryPath);
  execFileSync("tar", ["-xzf", archivePath, "-C", destDir], {
    stdio: "inherit",
  });
  checkOnlyFilesAndDirs(destDir);
}

async function main() {
  const tag = process.argv[2];
  if (!tag) {
    console.error("Usage: node scripts/import-convex-auth-docs.js <tag>");
    process.exit(1);
  }
  const srcRoot = fs.mkdtempSync(path.join(os.tmpdir(), "convex-auth-docs-"));
  try {
    await importDocs(tag, srcRoot);
  } finally {
    fs.rmSync(srcRoot, { recursive: true, force: true });
  }

  try {
    execSync(`npm run spellcheck`, { stdio: ["inherit", "inherit"] });
    console.log(`✅ Spellcheck passed!`);
  } catch {
    console.log(`❌ Spellcheck failed!`);
    process.exit(1);
  }
}

async function importDocs(tag, srcRoot) {
  const assetUrl = `${releaseUrl}/${encodeURIComponent(tag)}/${assetName}`;
  console.log(`Downloading ${assetUrl}...`);
  const [archive, checksumFile] = await Promise.all([
    download(assetUrl),
    download(`${assetUrl}.sha256`),
  ]);
  // The checksum catches a corrupt or partial download; it comes from the same
  // release, so it doesn't authenticate the archive.
  const expected = checksumFile.toString("utf8").trim().split(/\s+/)[0];
  const actual = crypto.createHash("sha256").update(archive).digest("hex");
  if (actual !== expected) {
    throw new Error(`${assetName} has SHA-256 ${actual}, expected ${expected}`);
  }

  const archivePath = path.join(srcRoot, assetName);
  fs.writeFileSync(archivePath, archive);
  const fromDir = path.join(srcRoot, "docs");
  fs.mkdirSync(fromDir);
  extractTarGz(archivePath, fromDir);

  const manifest = JSON.parse(
    fs.readFileSync(path.join(fromDir, manifestName), "utf8"),
  );
  if (manifest.tag !== tag) {
    throw new Error(`the archive of ${tag} holds the docs of ${manifest.tag}`);
  }
  console.log(`Importing Convex Auth docs at ${tag} (${manifest.commit})`);
  if (!fs.existsSync(path.join(fromDir, srcApiDirName))) {
    throw new Error(`the archive of ${tag} has no ${srcApiDirName} directory`);
  }

  fs.rmSync(guidesDir, { recursive: true, force: true });
  fs.rmSync(apiDir, { recursive: true, force: true });
  copyDocs(fromDir, guidesDir, {
    prefix: guidesPrefix,
    exclude: [srcApiDirName, manifestName],
  });
  copyDocs(path.join(fromDir, srcApiDirName), apiDir, {});
  // sidebars.js labels and places this directory, as for the other API
  // references.
  fs.rmSync(path.join(apiDir, "_category_.json"), { force: true });
  // The index is the page of the "Convex Auth API" category, so its heading
  // matches that label rather than the generic title TypeDoc gives it.
  const apiIndex = path.join(apiDir, "index.md");
  const apiIndexContent = fs.readFileSync(apiIndex, "utf8");
  const retitled = apiIndexContent.replace(
    /^title: .*$/m,
    "title: Convex Auth API",
  );
  if (retitled === apiIndexContent) {
    throw new Error(`${apiIndex} has no title to replace`);
  }
  fs.writeFileSync(apiIndex, retitled);
  // The sidebar of docs/auth is autogenerated. The index.mdx is the page of
  // this category, at /auth/convex-auth.
  fs.writeFileSync(
    path.join(guidesDir, "_category_.json"),
    JSON.stringify(
      { label: "Convex Auth", position: 1, collapsed: true },
      null,
      2,
    ) + "\n",
  );

  console.log("Running prettier...");
  execSync(`npx prettier -w ${guidesDir} ${apiDir}`, { stdio: "inherit" });

  console.log(`✅ Docs imported and processed successfully!`);
}

main();
