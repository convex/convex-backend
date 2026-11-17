import os from "os";
import path from "path";
import * as Sentry from "@sentry/node";
import { z } from "zod";
import type { Cli, Places } from "./harness.js";
import {
  convexMarketplaces,
  type Marketplace,
  type Plugin,
  type Source,
} from "./marketplaces.js";
import {
  fetchGitHubJson,
  onPath,
  parseList,
  subfolders,
  versionOf,
} from "./utils.js";

/** Builds `getClaudeCodeLocations` for the given places, so tests can point it at fixtures. */
export const createGetClaudeCodeLocations =
  (places: Omit<Places, "applications">) => async (): Promise<Cli[]> => {
    const found: Omit<Cli, "version">[] = [
      ...onPath("claude", places.path).map((p) => ({
        source: "terminal" as const,
        path: p,
      })),
      // The desktop app keeps every version it downloaded, as
      // `<version>/<hash>/claude.app`, next to its own state files.
      ...subfolders(
        path.join(
          places.home,
          "Library",
          "Application Support",
          "Claude",
          "claude-code",
        ),
      )
        .flatMap(subfolders)
        .map((dir) => ({
          source: "desktop" as const,
          path: path.join(dir, "claude.app", "Contents", "MacOS", "claude"),
        })),
      ...subfolders(path.join(places.home, ".vscode", "extensions"))
        .filter((dir) =>
          path.basename(dir).startsWith("anthropic.claude-code-"),
        )
        .map((dir) => ({
          source: "vscode" as const,
          path: path.join(dir, "resources", "native-binary", "claude"),
        })),
    ];
    return Promise.all(
      found.map(async (cli) => ({
        ...cli,
        version: await versionOf(cli.path),
      })),
    );
  };

/** Every copy of Claude Code on this machine: the terminal CLI, the Claude desktop app's copies and the VS Code extension's copies. */
export const getClaudeCodeLocations = createGetClaudeCodeLocations({
  home: os.homedir(),
  path: process.env.PATH ?? "",
});

/** The newest Convex plugin a Claude Code user can install, which is the commit Anthropic's marketplace pins, not our main. */
export async function claudeCodeLatest(): Promise<Record<string, string>> {
  const marketplace = await fetchGitHubJson<{
    plugins: { name: string; source: { url: string; sha: string } }[];
  }>("anthropics/claude-plugins-official/main/.claude-plugin/marketplace.json");
  if (marketplace === null) return {};
  const source = marketplace.plugins.find((p) => p.name === "convex")?.source;
  if (source === undefined) {
    Sentry.captureMessage(
      "Anthropic's marketplace has no convex plugin",
      "error",
    );
    return {};
  }
  const repo = source.url
    .replace(/^https:\/\/github\.com\//, "")
    .replace(/\.git$/, "");
  const plugin = await fetchGitHubJson<{ version: string }>(
    `${repo}/${source.sha}/.claude-plugin/plugin.json`,
  );
  return plugin === null ? {} : { convex: plugin.version };
}

/** `claude plugin marketplace list --json` */
const claudeCodeMarketplaceList = z.array(
  z.union([
    z.object({
      name: z.string(),
      source: z.literal("github"),
      repo: z.string(),
    }),
    z.object({ name: z.string(), source: z.literal("git"), url: z.string() }),
    z.object({
      name: z.string(),
      source: z.literal("directory"),
      path: z.string(),
    }),
    // The list holds other people's marketplaces too, so a source type we
    // haven't seen is kept by its type name instead of rejecting the list.
    z.object({ name: z.string(), source: z.string() }),
  ]),
);

/** Where a Claude Code marketplace comes from. zod keeps only the fields of the shape that matched. */
function claudeCodeSource(
  m: z.infer<typeof claudeCodeMarketplaceList>[number],
): Source {
  if ("repo" in m) return { kind: "github", repo: m.repo };
  if ("url" in m) return { kind: "git", url: m.url };
  if ("path" in m) return { kind: "folder", path: m.path };
  return { kind: "other", type: m.source };
}

/** `claude plugin list --json` */
const claudeCodePluginList = z.array(
  z.object({
    id: z.string(),
    version: z.string(),
    scope: z.string(),
    enabled: z.boolean(),
    folderVersion: z.string().optional(),
  }),
);

/** Reads the plugins in `claude plugin list --json`, or returns null when it doesn't match. */
function claudeCodePlugins(pluginList: string): Plugin[] | null {
  const plugins = parseList(
    claudeCodePluginList,
    pluginList,
    "claude plugin list --json",
  );
  if (plugins === null) return null;
  return plugins.map((p) => {
    const [name, marketplace] = p.id.split("@");
    // A folder install runs from the folder, and `version` is the copy cached at install time.
    return {
      name,
      marketplace,
      version: p.folderVersion ?? p.version,
      scope: p.scope,
      enabled: p.enabled,
      cachedVersion:
        p.folderVersion !== undefined && p.folderVersion !== p.version
          ? p.version
          : undefined,
    };
  });
}

/** Reads Claude Code's Convex marketplaces from `claude plugin marketplace list --json` and `claude plugin list --json`, or returns null when either doesn't match. */
export function claudeCodeMarketplaces(
  marketplaceList: string,
  pluginList: string,
): Marketplace[] | null {
  const marketplaces = parseList(
    claudeCodeMarketplaceList,
    marketplaceList,
    "claude plugin marketplace list --json",
  );
  const plugins = claudeCodePlugins(pluginList);
  if (marketplaces === null || plugins === null) return null;
  return convexMarketplaces(
    marketplaces.map((m) => ({ name: m.name, source: claudeCodeSource(m) })),
    plugins,
  );
}

const pluginId = "convex@claude-plugins-official";
/** The install step that brings an installed plugin to the latest version. When it fails, the user keeps an old version. */
const upgradeStep = ["plugin", "update", pluginId, "--scope", "user"];

/** Claude Code as `npx convex plugins` installs and reads it. */
export const claudeCode = {
  id: "claudeCode" as const,
  name: "Claude Code",
  plugin: pluginId,
  upgradeStep,
  installSteps: [
    // Notes:
    //
    // 1. Claude Code updates its local copy of the marketplace when it
    //    installs, but `plugin install` skips a plugin that's already
    //    installed, so an old version stays until the plugin is updated.

    // Add the marketplace
    ["plugin", "marketplace", "add", "anthropics/claude-plugins-official"],
    // Install the plugin if it isn't installed yet
    ["plugin", "install", pluginId, "--scope", "user"],
    // Update an installed plugin to the latest version
    upgradeStep,
  ],
  plugins: claudeCodePlugins,
  marketplaces: claudeCodeMarketplaces,
  latest: claudeCodeLatest,
};
