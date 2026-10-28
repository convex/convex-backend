import os from "os";
import path from "path";
import { nodeFs } from "../../../bundler/fs.js";
import { logOutput } from "../../../bundler/log.js";
import { fetchAgentSkillsCatalog } from "../versionApi.js";

/** Finds folders named `skills` in a folder's children and grandchildren, without following links. */
function skillsFolders(dir: string, depth: number): string[] {
  let entries;
  try {
    entries = nodeFs.listDir(dir);
  } catch {
    return [];
  }
  return entries
    .filter((entry) => entry.isDirectory())
    .flatMap((entry) => {
      const child = path.join(dir, entry.name);
      if (entry.name === "skills") return [child];
      return depth > 1 ? skillsFolders(child, depth - 1) : [];
    });
}

/** Convex skill names from the catalog on version.convex.dev, split into current and deleted ones. */
type SkillNames = { current: Set<string>; deleted: Set<string> };

/** A skills folder holding Convex skills. */
type SkillsFolder = {
  path: string;
  current: number;
  deleted: number;
  brokenLinks: number;
};

/** Finds the skills folders in a root's hidden folders that hold Convex skills, counting deleted skills and broken links. */
export function convexSkillsFolders(
  root: string,
  names: SkillNames,
): SkillsFolder[] {
  return nodeFs
    .listDir(root)
    .filter((entry) => entry.isDirectory() && entry.name.startsWith("."))
    .flatMap((entry) => skillsFolders(path.join(root, entry.name), 2))
    .map((folder) => {
      const entries = nodeFs.listDir(folder).map((entry) => entry.name);
      const current = entries.filter((name) => names.current.has(name));
      const deleted = entries.filter((name) => names.deleted.has(name));
      // exists follows links, so it's false for a link whose target is gone.
      const brokenLinks = [...current, ...deleted].filter(
        (name) => !nodeFs.exists(path.join(folder, name)),
      ).length;
      return {
        path: folder,
        current: current.length,
        deleted: deleted.length,
        brokenLinks,
      };
    })
    .filter((folder) => folder.current + folder.deleted > 0);
}

/** Prints each skills folder in a root's hidden folders that holds Convex skills, counting deleted skills and broken links. */
function logSkills(
  title: string,
  root: string,
  label: string,
  names: SkillNames,
) {
  const folders = convexSkillsFolders(root, names);
  logOutput(title);
  if (folders.length === 0) {
    logOutput("  No Convex skills found");
    return;
  }
  for (const folder of folders) {
    logOutput(
      `  ${folder.path.replace(root, label)}  ${folder.current} Convex skills, ${folder.deleted} deleted, ${folder.brokenLinks} broken links`,
    );
  }
}

/** Prints the Convex skills in the hidden folders of the home folder and the current folder. */
export async function logConvexSkills() {
  const catalog = await fetchAgentSkillsCatalog();
  if (catalog.kind === "error") {
    logOutput("Skills: couldn't fetch the list of Convex skills");
  } else {
    const names = (kind: "active" | "deleted") =>
      new Set(
        catalog.data.skills
          .filter((skill) => skill.status.kind === kind)
          .map((skill) => skill.skillName),
      );
    const skillNames = {
      current: names("active"),
      deleted: names("deleted"),
    };
    logSkills("Global skills", os.homedir(), "~", skillNames);
    logSkills("Project skills", process.cwd(), ".", skillNames);
  }
}
