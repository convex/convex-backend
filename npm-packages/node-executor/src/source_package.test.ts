import { createHash } from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { pathToFileURL } from "node:url";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { partialExtract } = vi.hoisted(() => ({
  partialExtract: { value: false },
}));

vi.mock("adm-zip", async () => {
  const { default: ActualAdmZip } =
    await vi.importActual<typeof import("adm-zip")>("adm-zip");
  return {
    default: function MockAdmZip(
      ...args: ConstructorParameters<typeof ActualAdmZip>
    ) {
      const zip = new ActualAdmZip(...args);
      const originalExtract = zip.extractAllTo.bind(zip);
      zip.extractAllTo = (outputDir: string, overwrite?: boolean) => {
        if (!partialExtract.value) {
          return originalExtract(outputDir, overwrite);
        }
        const metadata = zip.getEntry("metadata.json");
        if (metadata === null) {
          throw new Error("zip is missing metadata.json");
        }
        fs.mkdirSync(outputDir, { recursive: true });
        fs.writeFileSync(
          path.join(outputDir, "metadata.json"),
          metadata.getData(),
        );
      };
      return zip;
    },
  };
});

import AdmZip from "adm-zip";
import {
  availableSourcePackages,
  maybeDownloadAndLinkPackages,
  PackageRefcounts,
  type SourcePackage,
} from "./source_package";

const MODULE_PATH = "actions/hello.js";
const MODULE_SOURCE =
  'export default { isAction: true, invokeAction: async () => "{}" };\n';

function writeSourceZip(
  zipPath: string,
  modulePath: string,
  source: string,
): { sha256: string } {
  const zip = new AdmZip();
  const metadata = {
    modulePaths: [modulePath],
    moduleEnvironments: [[modulePath, "node"]],
  };
  zip.addFile("metadata.json", Buffer.from(JSON.stringify(metadata)));
  zip.addFile(`modules/${modulePath}`, Buffer.from(source));
  const zipBuffer = zip.toBuffer();
  fs.writeFileSync(zipPath, zipBuffer);
  return {
    sha256: createHash("sha256").update(zipBuffer).digest("base64url"),
  };
}

function sourcePackageFromZip(
  zipPath: string,
  key: string,
  sha256: string,
): SourcePackage {
  const uri = pathToFileURL(zipPath).href;
  return {
    uri,
    key,
    sha256,
    bundled_source: { uri, key, sha256 },
  };
}

describe("maybeDownloadAndLinkPackages", () => {
  let tmpDir: string;
  let tmpdirSpy: ReturnType<typeof vi.spyOn>;

  beforeEach(() => {
    partialExtract.value = false;
    tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "convex-source-package-"));
    tmpdirSpy = vi.spyOn(os, "tmpdir").mockReturnValue(tmpDir);
    availableSourcePackages.clear();
  });

  afterEach(() => {
    tmpdirSpy.mockRestore();
    availableSourcePackages.clear();
    fs.rmSync(tmpDir, { recursive: true, force: true });
  });

  it("should extract module files onto disk before caching the package", async () => {
    const zipPath = path.join(tmpDir, "pkg.zip");
    const { sha256 } = writeSourceZip(zipPath, MODULE_PATH, MODULE_SOURCE);
    const sourcePackage = sourcePackageFromZip(zipPath, "pkg-extract", sha256);
    const context = new PackageRefcounts();

    try {
      const local = await maybeDownloadAndLinkPackages(context, sourcePackage);
      const moduleFile = path.join(
        local.dir,
        "modules",
        ...MODULE_PATH.split("/"),
      );
      expect(fs.existsSync(moduleFile)).toBe(true);
      expect(fs.readFileSync(moduleFile, "utf-8")).toBe(MODULE_SOURCE);
      expect(local.modules.has(MODULE_PATH)).toBe(true);
    } finally {
      context.dispose();
    }
  });

  it("should not cache an extract that writes metadata.json but no module files", async () => {
    const zipPath = path.join(tmpDir, "pkg.zip");
    const { sha256 } = writeSourceZip(zipPath, MODULE_PATH, MODULE_SOURCE);
    const sourcePackage = sourcePackageFromZip(zipPath, "pkg-partial", sha256);
    partialExtract.value = true;

    const context = new PackageRefcounts();
    try {
      await expect(
        maybeDownloadAndLinkPackages(context, sourcePackage),
      ).rejects.toThrow(/missing modules\/actions\/hello\.js on disk/);
      expect(availableSourcePackages.has(sourcePackage.key)).toBe(false);
    } finally {
      context.dispose();
    }
  });

  it("should re-download when a cached package directory is later emptied", async () => {
    const zipPath = path.join(tmpDir, "pkg.zip");
    const { sha256 } = writeSourceZip(zipPath, MODULE_PATH, MODULE_SOURCE);
    const sourcePackage = sourcePackageFromZip(zipPath, "pkg-stale", sha256);

    const firstContext = new PackageRefcounts();
    let packageDir: string;
    try {
      const local = await maybeDownloadAndLinkPackages(
        firstContext,
        sourcePackage,
      );
      packageDir = local.dir;
      expect(
        fs.existsSync(
          path.join(packageDir, "modules", ...MODULE_PATH.split("/")),
        ),
      ).toBe(true);
    } finally {
      firstContext.dispose();
    }

    expect(availableSourcePackages.has(sourcePackage.key)).toBe(true);
    fs.rmSync(packageDir, { recursive: true, force: true });
    expect(availableSourcePackages.has(sourcePackage.key)).toBe(true);

    const secondContext = new PackageRefcounts();
    try {
      const local = await maybeDownloadAndLinkPackages(
        secondContext,
        sourcePackage,
      );
      const moduleFile = path.join(
        local.dir,
        "modules",
        ...MODULE_PATH.split("/"),
      );
      expect(fs.existsSync(moduleFile)).toBe(true);
      expect(fs.readFileSync(moduleFile, "utf-8")).toBe(MODULE_SOURCE);
    } finally {
      secondContext.dispose();
    }
  });
});
