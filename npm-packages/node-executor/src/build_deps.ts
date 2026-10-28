import { FrameData } from "./errors";
import fs from "node:fs";
import { execSync } from "child_process";
import archiver from "archiver";
import { createHash } from "node:crypto";
import { Readable } from "node:stream";
import path from "node:path";
import os from "node:os";
import { fileURLToPath } from "node:url";
import { logDurationMs } from "./log";
import { Hash } from "crypto";

export type BuildDepsRequest = {
  type: "build_deps";
  requestId: string;

  deps: NodeDependency[];
  uploadUrl: string;
};

export type BuildDepsResponse =
  | {
      type: "success";
      sha256Digest: number[];
      zippedSizeBytes: number;
      unzippedSizeBytes: number;
    }
  | {
      type: "error";
      message: string;
      frames?: FrameData[];
    };

export type NodeDependency = {
  package: string;
  version: string;
};

export async function buildDeps(
  request: BuildDepsRequest,
): Promise<BuildDepsResponse> {
  const url = new URL(request.uploadUrl);
  try {
    return await buildDepsInner(url, request.deps);
  } catch (e: any) {
    // eslint-disable-next-line @typescript-eslint/no-unused-expressions
    e.stack;
    return {
      type: "error",
      message: e.message ?? "",
      frames: e.__frameData ? JSON.parse(e.__frameData) : [],
    };
  }
}

export async function hashFromFile(file: string): Promise<Hash> {
  const hashReadStream = fs.createReadStream(file);
  const hash = createHash("sha256");

  hashReadStream.on("data", (data) => {
    hash.update(data);
  });

  return await new Promise((resolve, reject) => {
    hashReadStream
      .on("end", () => {
        resolve(hash);
      })
      .on("error", (err) => {
        reject(err);
      });
  });
}

// Taken from https://stackoverflow.com/questions/30448002/how-to-get-directory-size-in-node-js-without-recursively-going-through-directory
const dirSize = async (directory: string) => {
  const files = await fs.promises.readdir(directory);
  const stats: Promise<number>[] = files.map((file) => {
    const newPath = path.join(directory, file);
    return fs.promises.stat(newPath).then((stat) => {
      if (!stat.isDirectory()) {
        return stat.size;
      } else {
        return dirSize(newPath);
      }
    });
  });

  return (await Promise.all(stats)).reduce(
    (accumulator, size) => accumulator + size,
    0,
  );
};

async function buildDepsInner(
  url: URL,
  deps: NodeDependency[],
): Promise<BuildDepsResponse> {
  // Set working directory in /tmp since other directories are read-only in Lambda env.
  // Make sure the directory does not exist.
  const dir = path.join(os.tmpdir(), `build_deps`);
  fs.rmSync(dir, { recursive: true, force: true });
  fs.mkdirSync(dir, { recursive: true, mode: 0o744 });

  // file: is used for local backends that are using local node rather than lambda to execute.
  if (url.protocol !== "file:") {
    // Set npm cache directory
    process.env.NPM_CONFIG_CACHE = `${dir}/.npm`;
    // NPM config values are case-insensitive but some libraries rely on using one env var over the other.
    // Ex. Sharp, the image-processing library, relies on using process.env.npm_config_cache
    // as a build cache directory for libvips, but will not work with process.env.NPM_CONFIG_CACHE.
    process.env.npm_config_cache = `${dir}/.npm`;
  }

  // Create package.json with the dependencies as entries
  const deps_json = Object.fromEntries(deps.map((v) => [v.package, v.version]));
  const package_json = {
    name: url.toString(),
    version: "0.0.0",
    dependencies: deps_json,
  };

  // Write package.json
  fs.writeFileSync(`${dir}/package.json`, JSON.stringify(package_json));

  // Run "npm install"
  const startInstall = performance.now();
  execSync("npm install", { cwd: dir });
  logDurationMs("npm install", startInstall);

  // Zip generated "node_modules"
  const startZip = performance.now();
  if (!fs.existsSync(`${dir}/node_modules`)) {
    throw new Error("Failed to generate node_modules");
  }
  const output = fs.createWriteStream(`${dir}/node_modules.zip`);
  const zip = archiver("zip");
  const stream = zip.pipe(output);
  zip.directory(`${dir}/node_modules`, "node_modules");
  zip.finalize();
  await new Promise((resolve, _reject) => {
    stream
      .on("finish", () => {
        resolve(null);
      })
      .on("error", (err) => {
        throw err;
      });
  });
  logDurationMs("buildDepsZipDone", startZip);

  // Calculate sha256 digest of zip file
  const hash = await hashFromFile(`${dir}/node_modules.zip`);
  const buffer = hash.digest();
  const hashArray = Uint8Array.from(buffer);

  // Calculate file sizes
  const unzippedSizeBytes = await dirSize(`${dir}/node_modules`);
  const zippedSizeBytes = fs.statSync(`${dir}/node_modules.zip`).size;

  // Upload "node_modules.zip" to specified url
  let readStream: fs.ReadStream;
  const startUpload = performance.now();
  switch (url.protocol) {
    // This case is used for local backends that use the filesystem instead of S3 as a storage layer
    case "file:": {
      const filePath = fileURLToPath(url);
      fs.mkdirSync(path.dirname(filePath), {
        recursive: true,
        mode: 0o744,
      });
      fs.renameSync(`${dir}/node_modules.zip`, filePath);
      break;
    }
    // This is the S3 case
    case "https:":
      readStream = fs.createReadStream(`${dir}/node_modules.zip`);
      await fetch(url, {
        method: "PUT",
        headers: {
          "Content-Length": zippedSizeBytes.toString(),
        },
        // @ts-expect-error incompatible ReadableStream definitions
        body: Readable.toWeb(readStream),
        duplex: "half",
      });
      break;
    default:
      throw new Error(
        `unrecognized protocol ${url.protocol} for buildDeps upload url`,
      );
  }
  logDurationMs("externalPackageUpload", startUpload);

  return {
    type: "success",
    sha256Digest: Array.from(hashArray),
    unzippedSizeBytes,
    zippedSizeBytes,
  };
}
