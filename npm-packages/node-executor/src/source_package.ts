import { CanonicalizedModulePath } from "./convex";
import * as fs from "node:fs";
import * as stream from "node:stream";
import os from "node:os";
import path from "node:path";
import AdmZip from "adm-zip";
import concat from "concat-stream";

import { createHash } from "node:crypto";
import { logDebug, logDurationMs } from "./log";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";

export type SourcePackage = {
  // Deprecated fields
  uri: string;
  key: string;
  sha256: string;

  bundled_source: Package;
  external_deps?: Package | null;
};

export type Package = {
  uri: string;
  key: string;
  sha256: string;
};

type ModuleEnvironment = "node" | "isolate";

type MetadataJson = {
  modulePaths: string[];
  moduleEnvironments: Map<string, ModuleEnvironment>;
  externalDepsStorageKey?: string;
};

async function download(uri: string): Promise<stream.Readable> {
  const url = new URL(uri);
  if (url.protocol === "file:") {
    return fs.createReadStream(fileURLToPath(uri));
  } else {
    const response = await fetch(uri);
    if (!response.ok) {
      throw new Error(`Failed to fetch ${uri}: ${response.statusText}`);
    }
    // incompatible ReadableStream definitions
    return stream.Readable.fromWeb(response.body! as any);
  }
}

function parseMetadataFile(contents: string): MetadataJson {
  const metadataJson = JSON.parse(contents.toString()) as {
    modulePaths: string[];
    moduleEnvironments: [string, ModuleEnvironment][] | undefined;
    externalDepsStorageKey?: string;
  };

  metadataJson.modulePaths.sort();

  // Old versions didn't populate moduleEnvironments.
  if (metadataJson.moduleEnvironments === undefined) {
    metadataJson.moduleEnvironments = [];
    for (const path of metadataJson.modulePaths) {
      const environment = path.startsWith("actions/") ? "node" : "isolate";
      metadataJson.moduleEnvironments.push([path, environment]);
    }
  }

  const moduleEnvironmentsMap = new Map();
  for (const [path, environment] of metadataJson.moduleEnvironments) {
    moduleEnvironmentsMap.set(path, environment);
  }

  return {
    modulePaths: metadataJson.modulePaths,
    moduleEnvironments: moduleEnvironmentsMap,
    externalDepsStorageKey: metadataJson.externalDepsStorageKey,
  };
}

// In-flight `removeDir` promises
const pendingRemovals = new Map<string, Promise<void>>();

// Removes `dir`, serialized against any other removal of the same directory.
function removeDir(dir: string): Promise<void> {
  const removal = (pendingRemovals.get(dir) ?? Promise.resolve())
    .then(() => fs.promises.rm(dir, { recursive: true, force: true }))
    .catch((e) => {
      // Removals are best effort, just log failures
      logDebug(`Failed to remove ${dir}: ${e}`);
    })
    .finally(() => {
      if (pendingRemovals.get(dir) === removal) {
        pendingRemovals.delete(dir);
      }
    });
  pendingRemovals.set(dir, removal);
  return removal;
}

/**
 * Returns the cache entry for `key`, starting the download if it isn't cached
 * yet. Concurrent requests for the same key share a single download.
 *
 * This runs to completion synchronously so that callers can register their
 * interest in a package (via the refcount) before yielding to other requests.
 */
function getOrStartDownload<T>(
  context: PackageRefcounts,
  cache: Map<string, PackageCacheEntry<T>>,
  key: string,
  dir: string,
  startDownload: (dir: string) => Promise<T>,
): PackageCacheEntry<T> {
  const cached = cache.get(key);
  if (cached !== undefined) {
    return context.adopt(cached);
  }
  const entry: PackageCacheEntry<T> = {
    dir,
    refcount: 0,
    settled: false,
    ready: (pendingRemovals.get(dir) ?? Promise.resolve())
      // N.B.: .then() ensures `startDownload` runs after `adopt`, which bumps our own refcount
      .then(() => startDownload(dir))
      .catch(async (e) => {
        // Don't cache failures, so that the next request retries the download.
        if (cache.get(key) === entry) {
          cache.delete(key);
        }
        // No cleanup pass can reach the partial download once it's uncached.
        await removeDir(dir);
        throw e;
      })
      .finally(() => {
        entry.settled = true;
      }),
  };
  cache.set(key, entry);
  context.adopt(entry);
  return entry;
}

/// Downloads source package and external deps package, if necessary,
/// populating cache with result. Links external deps package into
/// local source package directory.
export async function maybeDownloadAndLinkPackages(
  context: PackageRefcounts,
  sourcePackage: SourcePackage,
): Promise<LocalSourcePackage> {
  const externalDeps = sourcePackage.external_deps ?? null;
  const external = externalDeps
    ? getOrStartDownload(
        context,
        availableExternalPackages,
        externalDeps.key,
        path.join(os.tmpdir(), `external_deps/${externalDeps.key}`),
        (dir) => downloadExternalPackage(dir, externalDeps),
      )
    : null;
  const source = getOrStartDownload(
    context,
    availableSourcePackages,
    sourcePackage.key,
    path.join(os.tmpdir(), `source/${sourcePackage.key}`),
    (dir) =>
      downloadAndLinkSourcePackage(dir, sourcePackage.bundled_source, external),
  );

  const [modules] = await Promise.all([source.ready, external?.ready]);
  return { dir: source.dir, modules };
}

async function downloadAndLinkSourcePackage(
  dir: string,
  bundledSource: Package,
  externalPackage: PackageCacheEntry<void> | null,
): Promise<Set<CanonicalizedModulePath>> {
  const modules = await downloadSourcePackage(dir, bundledSource);

  // Do symlinking of external package into local source package node_modules folder.
  //
  // This symlink is necessary only after the initial download, as a given
  // source package can only ever map to one set of external deps.
  // If external deps change, a new source package is created.
  if (externalPackage) {
    logDebug(
      `Attempting symlink from ${externalPackage.dir}/node_modules to ${dir}/node_modules`,
    );
    await fs.promises.symlink(
      `${externalPackage.dir}/node_modules`,
      `${dir}/node_modules`,
      "dir",
    );
  }

  return modules;
}

// Downloads sourcePackage and unzips it into `source/${sourcePackage.key}/modules`
async function downloadSourcePackage(
  dir: string,
  sourcePackage: Package,
): Promise<Set<CanonicalizedModulePath>> {
  const start = performance.now();
  logDebug("Downloading source package...");

  // First cleanup any previously downloaded packages (that are not still being used)
  // in order to not run out of space.
  await cleanupSourcePackages();
  logDurationMs("cleanupTime", start);

  // Create directory and do download in parallel
  const dirPromise = createFreshDir(dir);
  const downloadPackagePromise = download(sourcePackage.uri);
  const [sourcePackageStream, ..._] = await Promise.all([
    downloadPackagePromise,
    dirPromise,
  ]);

  // Process local source package
  const result = await processSourcePackageStream(
    dir,
    sourcePackage,
    sourcePackageStream,
  );
  logDurationMs("sourceDownloadTime", start);

  return result;
}

// Downloads externalPackage and unzips it into `externals/${externalPackage.key}/node_modules`.
async function downloadExternalPackage(
  dir: string,
  externalPackage: Package,
): Promise<void> {
  const start = performance.now();
  logDebug("External Package not available locally");

  // Cleanup other external dependency packages to not run out of disk space
  await cleanupExternalPackages();
  logDurationMs("cleanupExternalPackages", start);

  // Create directory and do download in parallel
  const downloadStart = performance.now();
  const dirPromise = createFreshDir(dir);
  const downloadPackagePromise = download(externalPackage.uri);
  const [externalPackageStream, ..._] = await Promise.all([
    downloadPackagePromise,
    dirPromise,
  ]);
  logDurationMs("downloadExternalsTime", downloadStart);

  // Process the external package download readable stream by checking hash and writing to dir
  await processExternalPackageStream(
    dir,
    externalPackage,
    externalPackageStream,
  );

  logDurationMs("externalDepsProcessingTime", start);
}

async function createFreshDir(dir: string) {
  await fs.promises.rm(dir, { recursive: true, force: true });
  // fs.promises.mkdir sometimes fails with ENOENT (the real error might be ENOSPC, but we're not sure)
  fs.mkdirSync(dir, { recursive: true, mode: 0o744 });
}

async function streamToBuffer(
  readableStream: stream.Readable,
): Promise<Buffer> {
  return new Promise((resolve, reject) => {
    // Use concat-stream to collect the stream data into a single buffer
    const concatStream = concat((data) => {
      resolve(data);
    });

    // Handle the stream with pipeline
    stream.promises.pipeline(readableStream, concatStream).catch(reject);
  });
}

async function processPackageStream(
  sha256Digest: string,
  packageStream: stream.Readable,
): Promise<Buffer> {
  // Create hashing stream
  const hash = createHash("sha256");
  packageStream.on("data", (chunk) => hash.update(chunk));
  const hashDone = new Promise((resolve, reject) => {
    packageStream
      .on("end", () => {
        resolve(null);
      })
      .on("error", (err) => {
        reject(err);
      });
  });

  const bufWriteDone = streamToBuffer(packageStream);

  // Make sure that all promises have been populated and the hash is done calculating
  const [buf, _] = await Promise.all([bufWriteDone, hashDone]);

  // Assert checksum matches
  const digest = hash.digest().toString("base64url");
  if (digest !== sha256Digest) {
    throw new Error(
      `Invalid checksum, got ${digest}, expected ${sha256Digest}`,
    );
  }

  return buf;
}

async function unzipFile(
  zipBuffer: Buffer,
  outputDir: string,
  entryValidator: ((entry: AdmZip.IZipEntry) => void) | null,
) {
  const zip = new AdmZip(zipBuffer);
  zip.extractAllTo(outputDir, true);

  const results = [];
  for (const zipEntry of zip.getEntries()) {
    if (entryValidator) entryValidator(zipEntry);
    results.push(zipEntry.entryName);
  }
  return results;
}

async function processExternalPackageStream(
  dir: string,
  externalPackage: Package,
  externalStream: stream.Readable,
): Promise<void> {
  const entryValidator = (entry: AdmZip.IZipEntry) => {
    if (!entry.entryName.startsWith("node_modules/")) {
      throw new Error(`found incorrect package entry ${entry.entryName}`);
    }
  };

  const startUnzip = performance.now();
  const zipBuffer = await processPackageStream(
    externalPackage.sha256,
    externalStream,
  );
  logDurationMs("unzipExternalsTime", startUnzip);

  const startWrites = performance.now();
  await unzipFile(zipBuffer, dir, entryValidator);
  logDurationMs("externalsWritesTime", startWrites);
}

type LocalSourcePackage = {
  dir: string;
  /**
   * The modules included in the package and that could contain Convex functions.
   * This doesn’t include bundler chunks (files in /_deps/).
   */
  modules: Set<CanonicalizedModulePath>;
};

/**
 * A package directory that is being, or has been, downloaded. The entry is
 * cached from the moment the download starts, so that concurrent requests for
 * the same package share one download.
 */
type PackageCacheEntry<T> = {
  dir: string;
  refcount: number;
  // true if `ready` has settled
  settled: boolean;
  ready: Promise<T>;
};

async function processSourcePackageStream(
  dir: string,
  sourcePackage: Package,
  sourceStream: stream.Readable,
): Promise<Set<CanonicalizedModulePath>> {
  const startUnzip = performance.now();
  const zipBuffer = await processPackageStream(
    sourcePackage.sha256,
    sourceStream,
  );
  logDurationMs("unzipSourceTime", startUnzip);

  // After finishing the pipeline, await on each File's buffer.
  const startWrites = performance.now();
  const entries = await unzipFile(zipBuffer, dir, null);
  const actualModulePaths = entries
    .filter(
      (entry) =>
        entry !== "metadata.json" &&
        // Some ZIP implementations store entries for directories themselves
        // (https://unix.stackexchange.com/a/743512/485280)
        // The Rust implementation we use in production doesn’t do it, but some
        // implementations (including the `archiver` npm package used in
        // node-executor integration tests) do so, so we are filtering them out.
        !entry.endsWith("/"),
    )
    .map((entry) => entry.substring("modules/".length));
  await fs.promises.chmod(`${dir}/metadata.json`, "444");
  const metadataJson = parseMetadataFile(
    await fs.promises.readFile(`${dir}/metadata.json`, {
      encoding: "utf-8",
    }),
  );

  // Old packages don't have project.json
  createPackageJsonIfMissing(dir);
  logDurationMs("sourceWritesTime", startWrites);
  actualModulePaths.sort();
  if (
    JSON.stringify(metadataJson.modulePaths) !==
    JSON.stringify(actualModulePaths)
  ) {
    throw new Error(
      `metadata.json incorrect. ${JSON.stringify(
        metadataJson.modulePaths,
      )}\n !=\n ${JSON.stringify(actualModulePaths)}`,
    );
  }

  return modulesFromMetadataJson(metadataJson);
}

export const availableSourcePackages = new Map<
  string,
  PackageCacheEntry<Set<CanonicalizedModulePath>>
>();
export const availableExternalPackages = new Map<
  string,
  PackageCacheEntry<void>
>();

/**
 * Prepopulates source and external deps caches if this Lambda was pushed with source and, optionally,
 * an external deps package.
 *
 * This source is pushed in ${__dirname}/source/ and includes the following folders:
 * - modules/ storing all user code
 * - [optional] node_modules/ storing external dependencies
 * - metadata.json storing MetadataJson object
 * - package.json
 */
export async function populatePrebuildPackages() {
  const sourceDir = path.join(__dirname, "/source");
  if (fs.statSync(sourceDir, { throwIfNoEntry: false }) === undefined) {
    // If we weren't pushed with source, skip prepopulations
    return;
  }

  const pkgs = fs.readdirSync(sourceDir);
  for (const pkg of pkgs) {
    const pkgDir = path.join(sourceDir, pkg);
    let metadata: MetadataJson;
    let modules: Set<string>;
    try {
      metadata = parseMetadataFile(
        fs.readFileSync(`${pkgDir}/metadata.json`, { encoding: "utf-8" }),
      );
      modules = modulesFromMetadataJson(metadata);
    } catch (e: any) {
      logDebug(`Failed to parse metadata.json during prebuild, skipping: ${e}`);
      continue;
    }
    availableSourcePackages.set(pkg, {
      dir: pkgDir,
      // Give this an infinite refcount so that it is never cleaned up
      refcount: Infinity,
      settled: true,
      ready: Promise.resolve(modules),
    });

    if (
      metadata.externalDepsStorageKey &&
      fs.existsSync(path.join(pkgDir, "node_modules"))
    ) {
      logDebug(
        `Prepopulating external deps with storage key ${metadata.externalDepsStorageKey} in ${pkgDir}`,
      );
      availableExternalPackages.set(metadata.externalDepsStorageKey, {
        dir: pkgDir,
        refcount: Infinity,
        settled: true,
        ready: Promise.resolve(),
      });
    }
  }
}

export function createPackageJsonIfMissing(dir: string) {
  // Ensure package.json exists. This is required so node knowns to execute
  // the user modules as ESM, since they have .js and not .mjs extension.
  const packageJsonPath = path.join(dir, "package.json");
  if (fs.existsSync(packageJsonPath)) {
    return;
  }
  fs.writeFileSync(packageJsonPath, `{ "type": "module" }`);
}

function modulesFromMetadataJson(
  metadataJson: MetadataJson,
): Set<CanonicalizedModulePath> {
  const modules: Set<string> = new Set();
  for (const path of metadataJson.modulePaths) {
    if (path.startsWith("_deps/")) {
      // Ignore bundler chunks since they don’t contain Convex function definitions.
      continue;
    } else if (path.endsWith(".js")) {
      // Only load node files.
      const environment = metadataJson.moduleEnvironments.get(path);
      if (!environment) {
        throw new Error(`Missing environment for ${path}`);
      }
      if (environment !== "node") {
        continue;
      }
      modules.add(path);
    } else if (path.endsWith(".js.map")) {
      continue;
    } else {
      throw new Error(`Invalid path in archive: ${path}`);
    }
  }
  return modules;
}

// Delete all unreferenced source packages.
export async function cleanupSourcePackages() {
  for (const [key, local] of availableSourcePackages) {
    if (local.settled && local.refcount === 0) {
      availableSourcePackages.delete(key);
      await removeDir(local.dir);
    }
  }
}

// Delete all unreferenced external dependency packages.
export async function cleanupExternalPackages() {
  for (const [key, pkg] of availableExternalPackages) {
    if (pkg.settled && pkg.refcount === 0) {
      availableExternalPackages.delete(key);
      await removeDir(pkg.dir);
    }
  }
}

export class PackageRefcounts {
  packages: { refcount: number }[];
  constructor() {
    this.packages = [];
  }
  adopt<T extends { refcount: number }>(pkg: T): T {
    pkg.refcount += 1;
    this.packages.push(pkg);
    return pkg;
  }
  // TODO: use Symbol.dispose when on Node 24+
  dispose() {
    for (const pkg of this.packages) {
      pkg.refcount -= 1;
    }
  }
}
