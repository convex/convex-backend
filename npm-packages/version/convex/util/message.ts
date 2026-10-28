import { Doc } from "../_generated/dataModel";
import { Chalk } from "chalk";
import * as semver from "semver";
import { extractVersionFromHeader } from "./convexClientHeader";

const chalk = new Chalk({
  level: 1, // Force chalk to output colors even in non-terminal environments
});

export function generateMessage(
  latestNpmVersion: Doc<"npmVersion">,
  convexClientHeader: string | null,
): string | null {
  const currentVersion = extractVersionFromHeader(convexClientHeader);

  if (!currentVersion) {
    return null;
  }

  const latestVersion = latestNpmVersion.value;

  // Parse versions using semver
  const current = semver.parse(currentVersion);
  const latest = semver.parse(latestVersion);

  if (!current || !latest) {
    return null;
  }

  // 1.33.1 was published after we forgot to bump some clients off 1.33.0.
  // Don't show an update nag for this specific patch transition.
  if (semver.eq(current, "1.33.0") && semver.eq(latest, "1.33.1")) {
    return null;
  }

  // Check if an update is available
  if (semver.gte(currentVersion, latestVersion)) {
    return null; // No update needed
  }

  // Determine update type
  const updateType = semver.diff(current, latest)!; // not equal ⇒ not null

  // Format the message with chalk
  const message = `${chalk.cyan(`A ${updateType} update is available for Convex`)} ${chalk.dim(`(${currentVersion} → ${latestVersion})`)}
${chalk.dim("Changelog:")} ${chalk.underline("https://github.com/convex/convex-js/blob/main/CHANGELOG.md#changelog")}`;

  return message;
}
