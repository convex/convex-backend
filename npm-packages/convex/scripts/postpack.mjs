// Remove internal types from the tarball produced by npm pack before publishing.

import url from "url";
import path from "path";
import { spawnSync } from "child_process";
import fs from "fs";
import * as tar from "tar";

const __dirname = url.fileURLToPath(new URL(".", import.meta.url));
const convexDir = path.join(__dirname, "..");

const tarball = getOnlyTarball(convexDir);

const tmpDir = path.join(
  convexDir,
  "tmpPackage" + ("" + Math.random()).slice(2, 8),
);
console.log("creating temp folder", tmpDir);
fs.rmSync(tmpDir, { force: true, recursive: true });
fs.mkdirSync(tmpDir);

await tar.extract({
  gzip: true,
  file: tarball,
  cwd: tmpDir,
  strict: true,
});
const tmpPackage = path.join(tmpDir, "package");

console.log("modifying package.json");
let packageJson = JSON.parse(
  fs.readFileSync(path.join(tmpPackage, "package.json")),
);
removeInternal(packageJson.exports);
pointToPublic(packageJson.typesVersions);

console.log("removing dev-only ts-node CLI script");
packageJson.bin["convex"] = packageJson.bin["convex-bundled"];
delete packageJson.bin["convex-bundled"];
delete packageJson.bin["//"];

fs.writeFileSync(
  path.join(tmpPackage, "package.json"),
  JSON.stringify(packageJson, null, 2) + "\n",
);

console.log("modifying stub directories");
const stubs = getStubDirectories(convexDir);
for (const [dirname, contents] of Object.entries(stubs)) {
  pointToPublic(contents);
  fs.writeFileSync(
    path.join(tmpPackage, dirname, "package.json"),
    JSON.stringify(contents, null, 2) + "\n",
  );
}

// Remove internal types
fs.rmSync(path.join(tmpPackage, "dist", "internal-cjs-types"), {
  recursive: true,
});
fs.rmSync(path.join(tmpPackage, "dist", "internal-esm-types"), {
  recursive: true,
});

// Remove `./bundle-analyzer-internal`
for (const dir of [
  "src",
  "dist/esm",
  "dist/cjs",
  "dist/esm-types",
  "dist/cjs-types",
]) {
  fs.rmSync(path.join(tmpPackage, dir, "bundle-analyzer-internal"), {
    recursive: true,
  });
}

auditInternal(tmpPackage);

await tar.create(
  {
    gzip: true,
    file: tarball,
    cwd: tmpDir,
    strict: true,
  },
  ["package"],
);
fs.rmSync(tmpDir, { recursive: true });

function getOnlyTarball(dirname) {
  const files = fs.readdirSync(dirname);
  const tarballs = files.filter((f) => f.endsWith(".tgz"));
  if (tarballs.length < 1) throw new Error("No tarball found.");
  if (tarballs.length > 1) {
    throw new Error(
      "Multiple tarballs found, please `rm *.tgz` and run again. `--pack-destination` is not allowed.",
    );
  }
  return path.join(dirname, tarballs[0]);
}

function removeInternal(obj) {
  for (const key of Object.keys(obj)) {
    const value = obj[key];
    if (key === "convex-internal-types") {
      delete obj[key];
    } else if (typeof value === "object") {
      removeInternal(value);
      // Clean up a key if it now points to an empty object
      if (Object.keys(value).length === 0) {
        delete obj[key];
      }
    }
  }
}

function pointToPublic(obj) {
  for (const key of Object.keys(obj)) {
    let value = obj[key];
    if (typeof value === "string") {
      value = value.replace("-internal", "").replace("internal-", "");
      obj[key] = value;
    }
    if (typeof value === "object") {
      pointToPublic(value);
    }
  }
}

function getStubDirectories(dirname) {
  return Object.fromEntries(
    fs
      .readdirSync(dirname)
      .filter((d) => fs.existsSync(path.join(dirname, d, "package.json")))
      .map((d) => [
        d,
        JSON.parse(
          fs.readFileSync(path.join(dirname, d, "package.json"), {
            encoding: "utf-8",
          }),
        ),
      ]),
  );
}

function run(...args) {
  console.log(args.join(" "));
  spawnSync(args[0], args.slice(1), { stdio: "inherit" });
}

// `tsc --removeInternal` works on methods of classes but not properties of
// objects or function parameters so until we move back to api-extractor we
// manually remove a list of @internal properties.
//
// To maintain declaration maps it's helpful to avoid changing line numbers.
function auditInternal(dirname) {
  auditForInternal(path.join(dirname, "dist", "cjs-types"));
  auditForInternal(path.join(dirname, "dist", "esm-types"));
}

// Assert that no `@internal` docstrings exist in types.
function auditForInternal(dir) {
  for (const entry of fs.readdirSync(dir)) {
    const file = path.join(dir, entry);
    if (fs.statSync(file)?.isDirectory()) {
      auditForInternal(file);
    } else if (file.endsWith(".d.ts")) {
      const contents = fs.readFileSync(file, { encoding: "utf-8" });
      const pattern =
        /\/\*\*(?<docstringContents>(\*(?!\/)|[^*])*@internal(\*(?!\/)|[^*])*)\*\/.*\n(?<nextLine>.*)\n/gm;
      const matches = [...contents.matchAll(pattern)];
      for (const [match] of matches) {
        console.log(
          "found @internal type in",
          file,
          `\n\`\`\`\n${match}\`\`\``,
        );
        throw new Error(
          "Found @internal type in published types! Until we switch to api-extractor you need to add a pattern for this in scripts/postpack.mjs in rewriteDtsToRemoveInternal().",
        );
      }
    }
  }
}
