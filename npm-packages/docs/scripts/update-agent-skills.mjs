#!/usr/bin/env node
// Regenerates the skills catalog rendered by src/components/AgentSkills.tsx
// from the convex/agent-skills repo.

import { execFileSync } from "node:child_process";
import {
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";

const SKILLS_REPO = "https://github.com/convex/agent-skills.git";
const OUTPUT_PATH = fileURLToPath(
  new URL("../src/data/agent-skills.json", import.meta.url),
);

// Only `name` and `description` are read, so this handles the subset of YAML the
// skill files use rather than pulling in a parser: single-line values, optionally
// double-quoted, and block values continued on indented lines.
function parseFrontmatter(source, skillDir) {
  const lines = source.split("\n");
  if (lines[0].trim() !== "---") {
    throw new Error(`${skillDir}: SKILL.md does not start with frontmatter`);
  }
  const end = lines.indexOf("---", 1);
  if (end === -1) {
    throw new Error(`${skillDir}: unterminated frontmatter`);
  }

  const fields = {};
  for (let i = 1; i < end; i++) {
    const match = /^([A-Za-z][\w-]*):[ \t]*(.*)$/.exec(lines[i]);
    if (match === null) {
      continue;
    }
    const [, key, inlineValue] = match;
    if (inlineValue !== "") {
      fields[key] = unquote(inlineValue.trim());
      continue;
    }
    const continuation = [];
    while (i + 1 < end && /^[ \t]+\S/.test(lines[i + 1])) {
      continuation.push(lines[++i].trim());
    }
    if (continuation.length === 0) {
      throw new Error(`${skillDir}: \`${key}\` has no value`);
    }
    fields[key] = unquote(continuation.join(" "));
  }
  return fields;
}

function unquote(value) {
  if (value.startsWith('"') && value.endsWith('"') && value.length >= 2) {
    return value.slice(1, -1).replaceAll('\\"', '"');
  }
  return value;
}

function readSkills(repoDir) {
  const skillsDir = join(repoDir, "skills");
  const dirs = readdirSync(skillsDir, { withFileTypes: true })
    .filter((entry) => entry.isDirectory())
    .map((entry) => entry.name)
    .sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));

  return dirs.map((dir) => {
    const source = readFileSync(join(skillsDir, dir, "SKILL.md"), "utf8");
    const { name, description } = parseFrontmatter(source, dir);
    if (!name || !description) {
      throw new Error(
        `${dir}: SKILL.md is missing \`name\` or \`description\``,
      );
    }
    if (name !== dir) {
      throw new Error(`${dir}: SKILL.md declares the name \`${name}\``);
    }
    return { name, description };
  });
}

// Upstream descriptions are written for the agent (trigger phrases, caveats)
// and run to several sentences; the first sentence is the part that reads as
// documentation.
function firstSentence(text) {
  return text.split(/(?<=[.!?])\s+/, 1)[0];
}

const repoDir = mkdtempSync(join(tmpdir(), "agent-skills-"));
let skills;
try {
  execFileSync("git", ["clone", "--depth", "1", SKILLS_REPO, repoDir], {
    stdio: ["ignore", "ignore", "inherit"],
  });
  skills = readSkills(repoDir);
} finally {
  rmSync(repoDir, { recursive: true, force: true });
}

writeFileSync(
  OUTPUT_PATH,
  JSON.stringify(
    {
      skills: skills.map(({ name, description }) => ({
        name,
        description: firstSentence(description),
      })),
    },
    null,
    2,
  ) + "\n",
);

console.error(`Wrote ${skills.length} skills.`);
