import { mkdir, mkdtemp, rm, writeFile } from "node:fs/promises";
import { tmpdir } from "node:os";
import path from "node:path";
import { expect, it, onTestFinished, vi } from "vitest";
import * as utils from "../cli/lib/utils/utils.js";
import { oneoffContext } from "./context.js";
import { bundle } from "./index.js";

const dependency = "@fixture/external";

async function installPackage(projectDir: string, version: string) {
  const packageDir = path.join(projectDir, "node_modules", dependency);
  await mkdir(packageDir, { recursive: true });
  await writeFile(
    path.join(packageDir, "package.json"),
    JSON.stringify({ name: dependency, version, type: "module" }),
  );
  await writeFile(path.join(packageDir, "index.js"), "export const value = 1;");
}

it.each([
  { layout: "hoisted", workspaceVersion: undefined, expectedVersion: "1.0.0" },
  {
    layout: "workspace-local",
    workspaceVersion: "2.0.0",
    expectedVersion: "2.0.0",
  },
])(
  "externalizes the $layout dependency",
  async ({ workspaceVersion, expectedVersion }) => {
    const root = await mkdtemp(path.join(tmpdir(), "convex-externals-"));
    onTestFinished(async () => {
      await rm(root, { recursive: true, force: true });
    });
    const workspace = path.join(root, "apps", "example");
    const convexDir = path.join(workspace, "convex");
    const entryPoint = path.join(convexDir, "action.js");
    await mkdir(convexDir, { recursive: true });
    await mkdir(path.join(workspace, "node_modules"));
    await writeFile(
      path.join(root, "package.json"),
      JSON.stringify({ private: true, workspaces: ["apps/*"] }),
    );
    await writeFile(
      path.join(workspace, "package.json"),
      JSON.stringify({ dependencies: { [dependency]: "*" } }),
    );
    await writeFile(
      entryPoint,
      `"use node"; export { value } from "${dependency}";`,
    );
    await installPackage(root, "1.0.0");
    if (workspaceVersion) {
      await installPackage(workspace, workspaceVersion);
    }

    const config = vi.spyOn(utils, "findParentConfigs").mockResolvedValue({
      parentPackageJson: path.join(workspace, "package.json"),
    });
    onTestFinished(() => {
      config.mockRestore();
    });
    const ctx = await oneoffContext({ skipAuth: true });
    const result = await bundle({
      ctx,
      dir: convexDir,
      entryPoints: [entryPoint],
      generateSourceMaps: false,
      platform: "node",
      externalPackagesAllowList: [dependency],
    });
    expect(result.externalDependencies).toEqual(
      new Map([[dependency, expectedVersion]]),
    );
    expect(result.bundledModuleNames.has(dependency)).toBe(false);
  },
);
