// Copies the "use rust" reqwest shim crate (crates/convex_reqwest in the
// convex-rs repo) into `rust/convex_reqwest` so it ships in the npm package
// (`rust` is listed in package.json "files"; the CLI points the managed
// `[patch.crates-io]` block at it). Only src/, Cargo.toml and v0_13/ are copied
// (not tests/, wasm/, compat/ or target/).
//
// Source: env CONVEX_REQWEST_SRC, default `../../../../crates/convex_reqwest`
// relative to this package (npm-packages/convex -> convex-rs root).
//
// Usage: node scripts/copy-reqwest-shim.mjs [--optional]
//   --optional: warn instead of failing when the source is missing.
import url from "url";
import path from "path";
import fs from "fs";

const packageDir = path.join(
  url.fileURLToPath(new URL(".", import.meta.url)),
  "..",
);

// The in-repo manifest depends on sibling crates by relative path, which does
// not exist once copied. Depend on the registry `convex_fn` instead (the CLI
// adds a `[patch.crates-io]` entry when the user's crate uses a path dep) and
// drop dev-dependencies.
export function standaloneManifest(toml) {
  return toml
    .replace(
      /^convex_fn\s*=\s*\{[^\n]*\}\s*$/m,
      'convex_fn = { version = "0.1", features = ["tokio-rt"] }',
    )
    .replace(/^\[dev-dependencies\][\s\S]*?(?=^# |^\[)/m, "");
}

export function copyReqwestShim({ optional = false } = {}) {
  const src = path.resolve(
    process.env.CONVEX_REQWEST_SRC ??
      path.join(packageDir, "..", "..", "..", "..", "crates", "convex_reqwest"),
  );
  const dest = path.join(packageDir, "rust", "convex_reqwest");
  if (!fs.existsSync(path.join(src, "Cargo.toml"))) {
    const msg = `reqwest shim source not found at ${src} (set CONVEX_REQWEST_SRC)`;
    if (optional) {
      console.warn(`warning: ${msg}; "use rust" reqwest support will not ship`);
      return false;
    }
    throw new Error(msg);
  }
  fs.rmSync(dest, { recursive: true, force: true });
  fs.mkdirSync(dest, { recursive: true });
  for (const entry of ["Cargo.toml", "src", "v0_13"]) {
    const from = path.join(src, entry);
    if (fs.existsSync(from)) {
      fs.cpSync(from, path.join(dest, entry), { recursive: true });
    }
  }
  for (const m of ["Cargo.toml", path.join("v0_13", "Cargo.toml")]) {
    const p = path.join(dest, m);
    if (fs.existsSync(p))
      fs.writeFileSync(p, standaloneManifest(fs.readFileSync(p, "utf8")));
  }
  console.log(`copied reqwest shim: ${src} -> ${dest}`);
  return true;
}

if (
  path.resolve(process.argv[1] ?? "") === url.fileURLToPath(import.meta.url)
) {
  copyReqwestShim({ optional: process.argv.includes("--optional") });
}
