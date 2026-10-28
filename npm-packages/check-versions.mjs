// Replacement for `rush check`: enforces that every dependency is requested
// with a consistent version range across all workspace projects, except for
// ranges listed in allowedAlternativeVersions.
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { execSync } from "node:child_process";

const root = path.dirname(fileURLToPath(import.meta.url));
const commonVersions = JSON.parse(
  stripJsonComments(
    fs.readFileSync(path.join(root, "common-versions.json"), "utf8"),
  ),
);
const allowed = commonVersions.allowedAlternativeVersions ?? {};

function stripJsonComments(s) {
  return s.replace(/\/\*[\s\S]*?\*\//g, "").replace(/^\s*\/\/.*$/gm, "");
}

const pnpm = path.join(root, "../scripts/node_modules/.bin/pnpm");
const projects = JSON.parse(
  execSync(`"${pnpm}" ls -r --depth -1 --json`, {
    cwd: root,
    maxBuffer: 1 << 26,
  }),
);

const requests = new Map(); // dep -> Map(range -> [projects])
for (const proj of projects) {
  const pkg = JSON.parse(
    fs.readFileSync(path.join(proj.path, "package.json"), "utf8"),
  );
  for (const section of ["dependencies", "devDependencies"]) {
    for (const [dep, range] of Object.entries(pkg[section] ?? {})) {
      if (range.startsWith("workspace:")) continue;
      if (!requests.has(dep)) requests.set(dep, new Map());
      const m = requests.get(dep);
      if (!m.has(range)) m.set(range, []);
      m.get(range).push(pkg.name);
    }
  }
}

let failed = false;
for (const [dep, ranges] of requests) {
  if (ranges.size <= 1) continue;
  // The "usual" range is the most-requested one; every other range must be
  // listed in allowedAlternativeVersions.
  const usual = [...ranges.entries()].reduce((a, b) =>
    b[1].length > a[1].length ? b : a,
  )[0];
  const offending = [...ranges.keys()].filter(
    (r) => r !== usual && !(allowed[dep] ?? []).includes(r),
  );
  if (offending.length > 0) {
    failed = true;
    process.stderr.write(
      `Inconsistent versions for "${dep}" (usual: ${usual}):\n`,
    );
    for (const r of offending) {
      process.stderr.write(`  ${r} — ${ranges.get(r).join(", ")}\n`);
    }
  }
}
if (failed) {
  process.stderr.write(
    "\nFound inconsistent dependency versions. Align them or add an entry to allowedAlternativeVersions in common-versions.json.\n",
  );
  process.exit(1);
}
process.stdout.write(
  `Checked ${requests.size} dependencies across ${projects.length} projects: consistent.\n`,
);
