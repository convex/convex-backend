import { version as npmVersion } from "../../version.js";
import AdmZip from "adm-zip";
import { Context } from "../../../bundler/context.js";
import {
  logFinishedStep,
  startLogProgress,
  logVerbose,
  logMessage,
  logError,
  logWarning,
} from "../../../bundler/log.js";
import {
  dashboardZip,
  executablePath,
  versionedBinaryDir,
  dashboardOutDir,
  resetDashboardDir,
  loadGlobalDashboardConfig,
  saveGlobalDashboardConfig,
  executableName,
} from "./filePaths.js";
import child_process from "child_process";
import { promisify } from "util";
import { Readable } from "stream";
import { TempPath, nodeFs, withTmpDir } from "../../../bundler/fs.js";
import { recursivelyDelete, recursivelyCopy } from "../fsUtils.js";
import { LocalDeploymentError } from "./errors.js";
import type { ProgressBarInstance } from "../../../vendor/progress/index.js";
import path from "path";
import { isValidBackendVersion } from "./utils.js";

async function makeExecutable(p: string) {
  switch (process.platform) {
    case "darwin":
    case "linux": {
      await promisify(child_process.exec)(`chmod +x ${p}`);
    }
  }
}

export async function ensureBackendBinaryDownloaded(
  ctx: Context,
  version:
    | { kind: "latest"; allowedVersion?: string | undefined }
    | { kind: "version"; version: string },
): Promise<{ binaryPath: string; version: string }> {
  if (version.kind === "version") {
    return _ensureBackendBinaryDownloaded(ctx, version.version);
  }
  if (version.allowedVersion) {
    const latestVersionWithBinary = await findLatestVersionWithBinary(
      ctx,
      false,
    );
    if (latestVersionWithBinary === null) {
      logWarning(
        `Failed to get latest version from GitHub, using downloaded version ${version.allowedVersion}`,
      );
      return _ensureBackendBinaryDownloaded(ctx, version.allowedVersion);
    }
    return _ensureBackendBinaryDownloaded(ctx, latestVersionWithBinary);
  }
  const latestVersionWithBinary = await findLatestVersionWithBinary(ctx, true);
  return _ensureBackendBinaryDownloaded(ctx, latestVersionWithBinary);
}

async function _ensureBackendBinaryDownloaded(
  ctx: Context,
  version: string,
): Promise<{ binaryPath: string; version: string }> {
  await assertValidBackendVersion(ctx, version);
  logVerbose(`Ensuring backend binary downloaded for version ${version}`);
  const existingDownload = await checkForExistingDownload(ctx, version);
  if (existingDownload !== null) {
    logVerbose(`Using existing download at ${existingDownload}`);
    return {
      binaryPath: existingDownload,
      version,
    };
  }
  const binaryPath = await downloadBackendBinary(ctx, version);
  return { version, binaryPath };
}

async function assertValidBackendVersion(ctx: Context, version: string) {
  if (!isValidBackendVersion(version)) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "Invalid local backend version",
      errForSentry: new LocalDeploymentError(
        `Invalid local backend version: ${JSON.stringify(version)}`,
      ),
    });
  }
}

let cachedLatestVersion: string | null = null;

/**
 * Finds the latest version of the Convex local backend through
 * version.convex.dev, caching the resolved version for the lifetime of the
 * process so repeated lookups hit the network at most once.
 *
 * Wraps {@link _findLatestVersionWithBinary}. Only successful (non-null)
 * results are cached, so a failed `requireSuccess: false` lookup can be retried
 * by a later call.
 */
export async function findLatestVersionWithBinary<
  RequireSuccess extends boolean,
>(
  ctx: Context,
  requireSuccess: RequireSuccess,
): Promise<RequireSuccess extends true ? string : string | null> {
  if (cachedLatestVersion !== null) {
    return cachedLatestVersion;
  }
  const result = await _findLatestVersionWithBinary(ctx, requireSuccess);
  if (result !== null) {
    cachedLatestVersion = result;
  }
  return result;
}

/**
 * Finds the latest version of the Convex local backend
 * through version.convex.dev
 */
export async function _findLatestVersionWithBinary<
  RequireSuccess extends boolean,
>(
  ctx: Context,
  requireSuccess: RequireSuccess,
): Promise<RequireSuccess extends true ? string : string | null> {
  // These shouldn't crash when there's a perfectly good binary already available.
  async function maybeCrash(
    ...args: Parameters<typeof ctx.crash>
  ): Promise<RequireSuccess extends true ? never : null> {
    if (requireSuccess) {
      return await ctx.crash(...args);
    }
    if (args[0].printedMessage) {
      logError(args[0].printedMessage);
    } else {
      logError("Error fetching latest version");
    }
    return null as RequireSuccess extends true ? never : null;
  }

  logVerbose("Fetching latest backend version from version API");

  try {
    const response = await fetch(
      "https://version.convex.dev/v1/local_backend_version",
      {
        headers: { "Convex-Client": `npm-cli-${npmVersion}` },
      },
    );

    if (!response.ok) {
      const text = await response.text();
      return await maybeCrash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: `version.convex.dev returned ${response.status}: ${text}`,
        errForSentry: new LocalDeploymentError(
          `version.convex.dev returned ${response.status}: ${text}`,
        ),
      });
    }

    const data = (await response.json()) as { version: string };
    if (!data.version) {
      return await maybeCrash({
        exitCode: 1,
        errorType: "fatal",
        printedMessage: "Invalid response missing version field",
        errForSentry: new LocalDeploymentError(
          "Invalid response missing version field",
        ),
      });
    }

    logVerbose(`Latest backend version is ${data.version}`);
    return data.version;
  } catch (e) {
    return maybeCrash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: "Failed to fetch latest backend version",
      errForSentry: new LocalDeploymentError(e?.toString()),
    });
  }
}

/**
 *
 * @param ctx
 * @param version
 * @returns The binary path if it exists, or null
 */
async function checkForExistingDownload(
  ctx: Context,
  version: string,
): Promise<string | null> {
  const destDir = versionedBinaryDir(version);
  if (!ctx.fs.exists(destDir)) {
    return null;
  }
  const p = executablePath(version);
  if (!ctx.fs.exists(p)) {
    // This directory isn't what we expected. Remove it.
    recursivelyDelete(ctx, destDir, { force: true });
    return null;
  }
  await makeExecutable(p);
  return p;
}

async function downloadBackendBinary(
  ctx: Context,
  version: string,
): Promise<string> {
  const downloadPath = getDownloadPath();
  // Note: We validate earlier that there's a binary for this platform at the specified version,
  // so in practice, we should never hit errors here.
  if (downloadPath === null) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `Unsupported platform ${process.platform} and architecture ${process.arch} for local deployment.`,
    });
  }
  await downloadZipFile(ctx, {
    version,
    filename: downloadPath,
    nameForLogging: "Convex backend binary",
    onDownloadComplete: async (ctx, unzippedPath) => {
      const name = executableName();
      const tempExecPath = path.join(unzippedPath, name);
      await makeExecutable(tempExecPath);
      logVerbose("Marked as executable");
      ctx.fs.mkdir(versionedBinaryDir(version), { recursive: true });
      ctx.fs.swapTmpFile(tempExecPath as TempPath, executablePath(version));
    },
  });
  return executablePath(version);
}

/**
 * Get the artifact name, composed of the target convex-local-backend and
 * the Rust "target triple" appropriate for the current machine.
 **/
function getDownloadPath() {
  switch (process.platform) {
    case "darwin":
      if (process.arch === "arm64") {
        return "convex-local-backend-aarch64-apple-darwin.zip";
      } else if (process.arch === "x64") {
        return "convex-local-backend-x86_64-apple-darwin.zip";
      }
      break;
    case "linux":
      if (process.arch === "arm64") {
        return "convex-local-backend-aarch64-unknown-linux-gnu.zip";
      } else if (process.arch === "x64") {
        return "convex-local-backend-x86_64-unknown-linux-gnu.zip";
      }
      break;
    case "win32":
      return "convex-local-backend-x86_64-pc-windows-msvc.zip";
  }
  return null;
}

function getGithubDownloadUrl(version: string, filename: string) {
  return `https://github.com/convex/convex-backend/releases/download/${version}/${filename}`;
}

async function downloadZipFile(
  ctx: Context,
  args: {
    version: string;
    filename: string;
    nameForLogging: string;
    onDownloadComplete: (ctx: Context, unzippedPath: TempPath) => Promise<void>;
  },
) {
  const { version, filename, nameForLogging } = args;
  const url = getGithubDownloadUrl(version, filename);
  const response = await fetch(url);
  const contentLength = parseInt(
    response.headers.get("content-length") ?? "",
    10,
  );
  let progressBar: ProgressBarInstance | null = null;
  if (!isNaN(contentLength) && contentLength !== 0 && process.stdout.isTTY) {
    progressBar = startLogProgress(
      `Downloading ${nameForLogging} [:bar] :percent :etas`,
      {
        width: 40,
        total: contentLength,
        clear: true,
      },
    );
  } else {
    logMessage(`Downloading ${nameForLogging}`);
  }
  if (response.status !== 200) {
    return await ctx.crash({
      exitCode: 1,
      errorType: "fatal",
      printedMessage: `File not found at ${url}.`,
    });
  }
  await withTmpDir(async (tmpDir) => {
    logVerbose(`Created tmp dir ${tmpDir.path}`);
    // Create a file in the tmp dir
    const zipLocation = tmpDir.registerTempPath(null);
    const readable = Readable.fromWeb(response.body! as any);
    await tmpDir.writeFileStream(zipLocation, readable, (chunk: any) => {
      if (progressBar !== null) {
        progressBar.tick(chunk.length);
      }
    });
    if (progressBar) {
      progressBar.terminate();
      logFinishedStep(`Downloaded ${nameForLogging}`);
    }
    logVerbose("Downloaded zip file");

    const zip = new AdmZip(zipLocation);
    await withTmpDir(async (versionDir) => {
      logVerbose(`Created tmp dir ${versionDir.path}`);
      zip.extractAllTo(versionDir.path, true);
      logVerbose("Extracted from zip file");
      await args.onDownloadComplete(ctx, versionDir.path);
    });
  });
  return executablePath(version);
}

export async function ensureDashboardDownloaded(ctx: Context, version: string) {
  await assertValidBackendVersion(ctx, version);
  const config = loadGlobalDashboardConfig(ctx);
  if (config !== null && config.version === version) {
    return;
  }
  await resetDashboardDir(ctx);
  await _ensureDashboardDownloaded(ctx, version);
  // Record which version is now in `dashboardOutDir()`
  // so we can skip the download next time.
  saveGlobalDashboardConfig(ctx, { version });
}
async function _ensureDashboardDownloaded(ctx: Context, version: string) {
  const zipLocation = dashboardZip();
  if (ctx.fs.exists(zipLocation)) {
    ctx.fs.unlink(zipLocation);
  }
  const outDir = dashboardOutDir();
  await downloadZipFile(ctx, {
    version,
    filename: "dashboard.zip",
    nameForLogging: "Convex dashboard",
    onDownloadComplete: async (ctx, unzippedPath) => {
      await recursivelyCopy(ctx, nodeFs, unzippedPath, outDir);
      logVerbose("Copied into out dir");
    },
  });
  return outDir;
}
