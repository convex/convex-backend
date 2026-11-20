import os from "os";
import path from "path";
import { z } from "zod";
import { nodeFs } from "../../../bundler/fs.js";
import type { Cli, Places } from "./harness.js";
import {
  convexMarketplaces,
  type Marketplace,
  type Plugin,
} from "./marketplaces.js";
import {
  fetchGitHubJson,
  onPath,
  parseList,
  subfolders,
  versionOf,
} from "./utils.js";

/** Builds `getCodexLocations` for the given places, so tests can point it at fixtures. */
export const createGetCodexLocations =
  (places: Places) => async (): Promise<Cli[]> => {
    // The ChatGPT app also has `codex-cli/bin/codex`, a script that runs this
    // binary. Running that script from another app sets off macOS's App
    // Management prompt, and running the binary doesn't.
    const desktop = path.join(
      places.applications,
      "ChatGPT.app",
      "Contents",
      "Resources",
      "codex-cli",
      "CodexCLI.app",
      "Contents",
      "MacOS",
      "codex",
    );
    const found: Omit<Cli, "version">[] = [
      ...onPath("codex", places.path).map((p) => ({
        source: "terminal" as const,
        path: p,
      })),
      ...(nodeFs.exists(desktop)
        ? [{ source: "desktop" as const, path: desktop }]
        : []),
      // The extension keeps its binary in a folder named for the platform, like
      // `bin/macos-aarch64`.
      ...subfolders(path.join(places.home, ".vscode", "extensions"))
        .filter((dir) => path.basename(dir).startsWith("openai.chatgpt-"))
        .flatMap((dir) => subfolders(path.join(dir, "bin")))
        .map((dir) => ({
          source: "vscode" as const,
          path: path.join(dir, "codex"),
        })),
    ];
    return Promise.all(
      found.map(async (cli) => ({
        ...cli,
        version: await versionOf(cli.path),
      })),
    );
  };

/** Every copy of Codex on this machine: the terminal CLI, the ChatGPT app's copy and the VS Code extension's copies. */
export const getCodexLocations = createGetCodexLocations({
  home: os.homedir(),
  path: process.env.PATH ?? "",
  applications: "/Applications",
});

/** The newest Convex plugin a Codex user can install, which is main of our repo. */
async function codexLatest(): Promise<Record<string, string>> {
  const plugin = await fetchGitHubJson<{ version: string }>(
    "convex/convex-codex-plugin/main/plugins/convex/.codex-plugin/plugin.json",
  );
  return plugin === null ? {} : { convex: plugin.version };
}

/** `codex plugin marketplace list --json` */
const codexMarketplaceList = z.object({
  marketplaces: z.array(
    z.object({
      name: z.string(),
      root: z.string(),
      marketplaceSource: z
        .object({ sourceType: z.string(), source: z.string() })
        .optional(),
    }),
  ),
});

/** `codex plugin list --json` */
const codexPluginList = z.object({
  installed: z.array(
    z.object({
      name: z.string(),
      marketplaceName: z.string(),
      version: z.string(),
      enabled: z.boolean(),
    }),
  ),
});

/** Reads the plugins in `codex plugin list --json`, or returns null when it doesn't match. */
function codexPlugins(pluginList: string): Plugin[] | null {
  const plugins = parseList(
    codexPluginList,
    pluginList,
    "codex plugin list --json",
  );
  if (plugins === null) return null;
  return plugins.installed.map((p) => ({
    name: p.name,
    marketplace: p.marketplaceName,
    version: p.version,
    enabled: p.enabled,
  }));
}

/** Reads Codex's Convex marketplaces from `codex plugin marketplace list --json` and `codex plugin list --json`, or returns null when either doesn't match. */
export function codexMarketplaces(
  marketplaceList: string,
  pluginList: string,
): Marketplace[] | null {
  const marketplaces = parseList(
    codexMarketplaceList,
    marketplaceList,
    "codex plugin marketplace list --json",
  );
  const plugins = codexPlugins(pluginList);
  if (marketplaces === null || plugins === null) return null;
  return convexMarketplaces(
    marketplaces.marketplaces.map((m) => ({
      name: m.name,
      // Anything that isn't a git marketplace is shown by its folder.
      source:
        m.marketplaceSource?.sourceType === "git"
          ? { kind: "git", url: m.marketplaceSource.source }
          : { kind: "folder", path: m.root },
    })),
    plugins,
  );
}

const pluginId = "convex@convex-codex-plugin";
/** The install step that brings an installed plugin to the latest version. When it fails, the user keeps an old version. */
const upgradeStep = ["plugin", "marketplace", "upgrade", "convex-codex-plugin"];

/** Codex as `npx convex plugins` installs and reads it. */
export const codex = {
  id: "codex" as const,
  name: "Codex",
  plugin: pluginId,
  upgradeStep,
  installSteps: [
    // Notes:
    //
    // 1. The marketplace is added from the `get-convex` org, the org's name
    //    before the rename to `convex`. Codex refuses to re-add a marketplace
    //    from a different source, so switching would make install fail for
    //    anyone who installed before. Switch once install handles an existing
    //    marketplace.
    //
    // 2. Codex installs plugins from its own local copy of each marketplace, so
    //    it doesn't see the new plugin versions we publish until that copy is
    //    updated.

    // Add the marketplace
    ["plugin", "marketplace", "add", "convex/convex-codex-plugin"],
    // Update the local copy from GitHub, so the latest plugin version is
    // available to install - this is important
    upgradeStep,
    // Install the latest plugin from the updated marketplace
    ["plugin", "add", pluginId],
  ],
  plugins: codexPlugins,
  marketplaces: codexMarketplaces,
  latest: codexLatest,
};
