/**
 * The Claude Code and Codex CLIs found on the user's machine. Users of the
 * desktop apps or the VS Code extensions often have no CLI on PATH, but each
 * app ships its own copy, which takes the same commands and shares the same
 * plugin state as the terminal CLI.
 */
import * as semver from "semver";
import { getClaudeCodeLocations } from "./claudeCode.js";
import { getCodexLocations } from "./codex.js";

/** Where a copy of a harness CLI was found. */
type CliSource = "terminal" | "desktop" | "vscode";

/** A copy of a harness CLI and the first line it prints for `--version`. */
export type Cli = { source: CliSource; path: string; version: string };

/** Where to look for harness CLIs. */
export type Places = {
  home: string;
  /** Directories separated like `$PATH`. */
  path: string;
  /** Where macOS apps are installed, `/Applications`. */
  applications: string;
};

/** The copy with the highest version. Versions compare as semver, where a release beats its prereleases. */
function newest(clis: Cli[]): Cli | undefined {
  // Every copy printed its version when the finder ran it.
  const version = (cli: Cli) =>
    semver.coerce(cli.version, { includePrerelease: true })!;
  return [...clis].sort((a, b) => semver.rcompare(version(a), version(b)))[0];
}

/** Builds `getNewestHarnessLocations` from the given location getters, so tests can pass fixed locations. */
export const createGetNewestHarnessLocations =
  (getters: {
    getCodexLocations: () => Promise<Cli[]>;
    getClaudeCodeLocations: () => Promise<Cli[]>;
  }) =>
  async () => ({
    codex: newest(await getters.getCodexLocations()),
    claudeCode: newest(await getters.getClaudeCodeLocations()),
  });

/** The newest copy of each harness's CLI on this machine, or `undefined` for a harness that isn't installed. */
export const getNewestHarnessLocations = createGetNewestHarnessLocations({
  getCodexLocations,
  getClaudeCodeLocations,
});
