import { logOutput } from "../../../bundler/log.js";
import { formatRows } from "./utils.js";

/** Names of the Convex plugins, including the old internal beta. */
const pluginNames = ["convex", "convex-beta"];

/** A Convex plugin as its harness reports it. */
export type Plugin = {
  name: string;
  marketplace: string;
  /** The version that runs. */
  version: string;
  /** Claude Code only. */
  scope?: string;
  enabled: boolean;
  /** Claude Code folder installs only. The copy cached at install time, when it differs from the folder. */
  cachedVersion?: string | undefined;
};

/** Where a marketplace comes from. `other` is a source type we haven't seen, by its type name. */
export type Source =
  | { kind: "github"; repo: string }
  | { kind: "git"; url: string }
  | { kind: "folder"; path: string }
  | { kind: "other"; type: string };

/** A marketplace named after Convex or holding a Convex plugin, with the Convex plugins installed from it. */
export type Marketplace = { name: string; source: Source; plugins: Plugin[] };

/** Keeps the marketplaces named after Convex or holding a Convex plugin, each with its Convex plugins. */
export function convexMarketplaces(
  marketplaces: { name: string; source: Source }[],
  plugins: Plugin[],
): Marketplace[] {
  const convex = plugins.filter((p) => pluginNames.includes(p.name));
  return marketplaces
    .map((m) => ({
      ...m,
      plugins: convex.filter((p) => p.marketplace === m.name),
    }))
    .filter((m) => m.name.startsWith("convex") || m.plugins.length > 0);
}

/** Describes a marketplace's source for people. */
function sourceText(source: Source): string {
  switch (source.kind) {
    case "github":
      return `github.com/${source.repo}`;
    case "git":
      return source.url.replace(/^https?:\/\//, "").replace(/\.git$/, "");
    case "folder":
      return source.path;
    case "other":
      return source.type;
  }
}

/** Prints each Convex marketplace with its Convex plugins indented under it. */
export function logMarketplaces(
  title: string,
  marketplaces: Marketplace[],
  latest: Record<string, string>,
) {
  logOutput(title);
  if (marketplaces.length === 0) {
    logOutput("  No Convex plugin installed");
    return;
  }
  const marketplaceLines = formatRows(
    marketplaces.map((m) => [m.name, sourceText(m.source)]),
  );
  // Plugin rows are formatted together so their columns line up across marketplaces.
  const pluginLines = formatRows(
    marketplaces
      .flatMap((m) => m.plugins)
      .map((p) => [
        p.name,
        p.version,
        latest[p.name] === undefined ? "" : `latest ${latest[p.name]}`,
        p.scope ?? "-",
        p.enabled ? "enabled" : "disabled",
        p.cachedVersion === undefined ? "" : `cached copy ${p.cachedVersion}`,
      ]),
    "    ",
  );
  let next = 0;
  marketplaces.forEach((m, i) => {
    logOutput(marketplaceLines[i]);
    for (const line of pluginLines.slice(next, next + m.plugins.length)) {
      logOutput(line);
    }
    next += m.plugins.length;
  });
}
