import fs from "fs";
import path from "path";
import { Build, BundleResult } from "./bundleResult.js";
import {
  BuildRecord,
  CollectedBundle,
  ExternalPackageInfo,
  InputAttribution,
  ModuleSizes,
  modulePathFromMetafileOutput,
} from "./model.js";
import { analyzerConvexVersion } from "./bundle.js";
import { InputSourceMaps, attributeModuleSourceMap } from "./sourceMap.js";

export function collectBundle(input: BundleResult): CollectedBundle {
  return {
    projectRoot: input.projectRoot,
    convexVersion: analyzerConvexVersion,
    includeSourcesContent: input.includeSourcesContent,
    builds: measureBuilds(input.projectRoot, input.builds),
  };
}

export function measureBuilds(root: string, builds: Build[]): BuildRecord[] {
  const attributor = new PackageAttributor(root);
  const inputSourceMaps = new InputSourceMaps(root);
  return builds.map((m) => {
    const attribution = new Map<string, InputAttribution>();
    for (const inputPath of Object.keys(m.metafile.inputs)) {
      attribution.set(
        inputPath,
        attributor.attribute(inputPath, m.component.directory),
      );
    }
    const externalPackages: ExternalPackageInfo[] = Object.entries(
      m.externalDependencies,
    ).map(([name, version]) => ({
      name,
      version,
      diskBytes: attributor.installedPackageBytes(name, m.component.directory),
    }));
    return {
      component: {
        definitionPath: m.component.definitionPath,
        label: m.component.label,
        isRoot: m.component.isRoot,
      },
      kind: m.kind,
      metafile: m.metafile,
      modules: measureModules(m, inputSourceMaps),
      attribution,
      externalPackages,
    };
  });
}

function measureModules(
  m: Build,
  inputSourceMaps: InputSourceMaps,
): Map<string, ModuleSizes> {
  const outputInputs = new Map<string, string[]>();
  for (const [key, output] of Object.entries(m.metafile.outputs)) {
    outputInputs.set(
      modulePathFromMetafileOutput(key),
      Object.keys(output.inputs),
    );
  }
  const modules = new Map<string, ModuleSizes>();
  for (const mod of m.modules) {
    const map = mod.sourceMap;
    const attribution =
      map === undefined
        ? {
            bytesByInput: new Map(),
            overheadBytes: 0,
            unmatchedBytesBySource: new Map(),
          }
        : attributeModuleSourceMap(
            map,
            mod.path,
            outputInputs.get(mod.path) ?? [],
            (input) => inputSourceMaps.originalSources(input),
          );
    modules.set(mod.path, {
      codeBytes: Buffer.byteLength(mod.source),
      sourceMapBytes: map === undefined ? 0 : Buffer.byteLength(map),
      sourceMapBytesByInput: attribution.bytesByInput,
      sourceMapOverheadBytes: attribution.overheadBytes,
      sourceMapUnmatchedBytesBySource: attribution.unmatchedBytesBySource,
    });
  }
  return modules;
}

type PackageJsonInfo = { dir: string; name: string; version: string | null };

/**
 * esbuild resolves symlinks, so package ownership follows package.json files
 * rather than node_modules path segments. Versionless manifests named after
 * an enclosing package's subpath belong to that enclosing package.
 */
class PackageAttributor {
  private packagesByDir = new Map<string, PackageJsonInfo | null>();
  private projectPackage: PackageJsonInfo | null;

  constructor(private root: string) {
    this.projectPackage = this.nearestPackage(path.join(root, "x"));
  }

  attribute(inputPath: string, componentDir: string): InputAttribution {
    const none = { packageName: null, packageVersion: null, packageDir: null };
    if (/^[\w-]+:/.test(inputPath) || inputPath.includes("(disabled):")) {
      return { origin: "virtual", ...none };
    }
    const abs = path.resolve(this.root, inputPath);
    const own = {
      origin: abs.split(path.sep).includes("_generated")
        ? ("generated" as const)
        : ("app" as const),
      ...none,
    };
    if (isWithin(abs, path.resolve(this.root, componentDir))) {
      return own;
    }
    const pkg = this.nearestPackage(abs);
    if (!pkg || pkg.dir === this.projectPackage?.dir) {
      return own;
    }
    return {
      origin: pkg.name === "convex" ? "convex" : "npm",
      packageName: pkg.name,
      packageVersion: pkg.version,
      packageDir: toPosix(path.relative(this.root, pkg.dir)),
    };
  }

  /** Resolves `name` from `fromDir` the way Node does and sizes its directory. */
  installedPackageBytes(name: string, fromDir: string): number | null {
    let dir = path.resolve(this.root, fromDir);
    while (true) {
      const candidate = path.join(dir, "node_modules", name);
      if (fs.existsSync(candidate)) {
        return directorySize(candidate);
      }
      const parent = path.dirname(dir);
      if (parent === dir) return null;
      dir = parent;
    }
  }

  private nearestPackage(file: string): PackageJsonInfo | null {
    return this.packageAbove(path.dirname(file));
  }

  private packageAbove(dir: string): PackageJsonInfo | null {
    const cached = this.packagesByDir.get(dir);
    if (cached !== undefined) return cached;
    const pkg = readNamedPackageJson(dir);
    const parent = path.dirname(dir);
    const installationRoot =
      path.basename(parent) === "node_modules" ||
      (path.basename(parent).startsWith("@") &&
        path.basename(path.dirname(parent)) === "node_modules");
    let found = pkg;
    if (!pkg || (!pkg.version && !installationRoot)) {
      const above = parent === dir ? null : this.packageAbove(parent);
      if (
        !pkg ||
        (above &&
          (pkg.name === above.name || pkg.name.startsWith(`${above.name}/`)))
      ) {
        found = above;
      }
    }
    this.packagesByDir.set(dir, found);
    return found;
  }
}

function readNamedPackageJson(dir: string): PackageJsonInfo | null {
  try {
    const json = JSON.parse(
      fs.readFileSync(path.join(dir, "package.json"), "utf8"),
    );
    if (typeof json.name !== "string") return null;
    return {
      dir,
      name: json.name,
      version: typeof json.version === "string" ? json.version : null,
    };
  } catch {
    return null;
  }
}

/**
 * Sums file sizes under `dir`, following `dir` itself if it's a symlink but
 * not symlinks inside it, which in pnpm installs point at other packages.
 */
export function directorySize(dir: string): number {
  let total = 0;
  const stack = [dir];
  while (stack.length > 0) {
    const current = stack.pop()!;
    let entries;
    try {
      entries = fs.readdirSync(current, { withFileTypes: true });
    } catch {
      continue;
    }
    for (const entry of entries) {
      const full = path.join(current, entry.name);
      if (entry.isDirectory()) {
        stack.push(full);
      } else if (entry.isFile()) {
        total += fs.statSync(full).size;
      }
    }
  }
  return total;
}

function isWithin(child: string, parent: string): boolean {
  const rel = path.relative(parent, child);
  return rel !== "" && !rel.startsWith("..") && !path.isAbsolute(rel);
}

function toPosix(p: string): string {
  return p.split(path.sep).join("/");
}
