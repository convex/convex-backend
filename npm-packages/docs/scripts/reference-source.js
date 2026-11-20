// Picks the commit of the `convex` package that the generated reference pages
// document, and logs which commit it is and why.
//
// CONVEX_DOCS_REFERENCE_SOURCE selects it:
//
// - "branch" (default): the working tree, with a banner on every generated
//   page saying that docs.convex.dev documents the latest release.
// - "release": the release of the version in `npm-packages/convex/package.json`,
//   so that unreleased changes don't reach docs.convex.dev before the package
//   does. Netlify's production deploys set it. The release is its
//   `npm/X.Y.Z` tag, fetched from `origin` if it's missing locally. A release
//   commit lands before its tag is pushed, so while releasing, the tag is
//   missing, and the release is the commit that last changed the version.
//
// Source links point at the documented commit. The repository is private, so
// release builds link to its public mirror, convex/convex-backend, which has
// the same paths.

const { execFileSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const { formatReferenceSourceBanner } = require("./reference-source-banner.js");

const REPO_ROOT = path.resolve(__dirname, "../../..");
const CONVEX_PACKAGE_DIR = "npm-packages/convex";
const PACKAGE_JSON = `${CONVEX_PACKAGE_DIR}/package.json`;
const SOURCE_ENV_VAR = "CONVEX_DOCS_REFERENCE_SOURCE";
// Holds the extracted release and the generated tsconfig. The release is laid
// out like the repository, so that paths relative to it are also paths in the
// GitHub repository.
const CACHE_DIR = path.resolve(__dirname, "../.reference-source");
const EXTRACT_ROOT = path.join(CACHE_DIR, "release");
const MANIFEST_PATH = path.join(EXTRACT_ROOT, "manifest.json");
// How many commits to fetch at a time when the commit that set the version is
// missing from a shallow clone.
const DEEPEN_BY = 500;
const MAX_DEEPEN_ATTEMPTS = 10;
const INTERNAL_REPO = "convex/convex";
const PUBLIC_REPO = "convex/convex-backend";
// A bare repository with only the commits of PUBLIC_REPO, to find the mirror of
// a commit by the `GitOrigin-RevId: <commit>` line Copybara adds.
const MIRROR_GIT_DIR = path.join(CACHE_DIR, "convex-backend.git");
const DAY_MS = 24 * 60 * 60 * 1000;

const BRANCH_BANNER = `:::warning

This is an internal build of the docs. This page is generated from the
\`convex\` package on this branch, which can include unreleased changes.
[docs.convex.dev](https://docs.convex.dev/) shows the latest release on npm. To
generate this page from the release instead, set
\`${SOURCE_ENV_VAR}=release\`.

:::`;

function printBanner(source) {
  // chalk is ESM-only; Node loads it with `require` since 22.12, the minimum
  // in package.json's `engines`.
  const { default: chalk } = require("chalk");
  console.log(
    formatReferenceSourceBanner(source, {
      chalk,
      hyperlinks: chalk.level > 0 && Boolean(process.stdout.isTTY),
    }),
  );
}

function git(...args) {
  return execFileSync("git", args, {
    cwd: REPO_ROOT,
    encoding: "utf8",
  }).trim();
}

function isShallow() {
  return git("rev-parse", "--is-shallow-repository") === "true";
}

// Returns whether the tag exists, fetching it from `origin` if it's missing
// locally. The repository has thousands of tags, and fetching tags into a
// shallow clone pulls their history, so this fetches only `tag`.
function ensureTag(tag) {
  if (git("tag", "--list", tag) === tag) {
    return true;
  }
  const ref = `refs/tags/${tag}`;
  if (git("ls-remote", "--tags", "--refs", "origin", ref) === "") {
    return false;
  }
  // `--depth` would make a full clone shallow at the tagged commit.
  git(
    "fetch",
    ...(isShallow() ? ["--depth=1"] : []),
    "--no-tags",
    "origin",
    `${ref}:${ref}`,
  );
  return true;
}

function shallowBoundaries() {
  const shallowFile = path.resolve(
    REPO_ROOT,
    git("rev-parse", "--git-path", "shallow"),
  );
  return fs.existsSync(shallowFile)
    ? fs.readFileSync(shallowFile, "utf8").split("\n").filter(Boolean)
    : [];
}

// In a shallow clone, `git log -- <path>` reports the boundary commit as
// changing every file, so this deepens the clone until the commit it finds is
// a real one.
function lastVersionCommit() {
  for (let attempt = 0; ; attempt++) {
    const commit = git(
      "log",
      "-1",
      "--format=%H",
      '-G"version":',
      "--",
      PACKAGE_JSON,
    );
    if (commit !== "" && !shallowBoundaries().includes(commit)) {
      return commit;
    }
    if (!isShallow()) {
      throw new Error(`No commit changes the version in ${PACKAGE_JSON}`);
    }
    if (attempt === MAX_DEEPEN_ATTEMPTS) {
      throw new Error(
        `No commit changes the version in ${PACKAGE_JSON} within the last ` +
          `${DEEPEN_BY * MAX_DEEPEN_ATTEMPTS} commits of this shallow clone`,
      );
    }
    git(
      "fetch",
      `--deepen=${DEEPEN_BY}`,
      "--no-tags",
      "origin",
      git("rev-parse", "HEAD"),
    );
  }
}

// Returns the commit of PUBLIC_REPO that mirrors `commit`, or null. Copybara
// mirrors `main` within minutes, and the mirror is newer than `commit`, so this
// fetches only the commit messages since a day before it.
function publicMirrorCommit(commit) {
  const mirrorGit = (...args) =>
    execFileSync("git", ["--git-dir", MIRROR_GIT_DIR, ...args], {
      encoding: "utf8",
    }).trim();
  const since = new Date(
    Number(git("log", "-1", "--format=%ct", commit)) * 1000 - DAY_MS,
  ).toISOString();
  try {
    if (!fs.existsSync(MIRROR_GIT_DIR)) {
      execFileSync("git", ["init", "--quiet", "--bare", MIRROR_GIT_DIR]);
    }
    mirrorGit(
      "fetch",
      "--quiet",
      "--filter=tree:0",
      `--shallow-since=${since}`,
      "--no-tags",
      `https://github.com/${PUBLIC_REPO}.git`,
      "main",
    );
  } catch (error) {
    console.warn(`⚠️  Reference docs: couldn't fetch ${PUBLIC_REPO}: ${error}`);
    return null;
  }
  return (
    mirrorGit(
      "log",
      "-1",
      "--format=%H",
      `--grep=^GitOrigin-RevId: ${commit}$`,
      "FETCH_HEAD",
    ) || null
  );
}

// Links to the public mirror of `commit`, or to the mirror's `main` (whose
// line numbers can differ) until it's mirrored.
function releaseSourceLinks(commit) {
  const mirror = publicMirrorCommit(commit);
  if (mirror === null) {
    console.warn(
      `⚠️  Reference docs: ${PUBLIC_REPO} has no mirror of ${commit} yet, so ` +
        `source links point at its main branch.`,
    );
  }
  return { mirrorRepo: PUBLIC_REPO, mirrorRef: mirror ?? "main" };
}

function versionAt(commit) {
  return JSON.parse(git("show", `${commit}:${PACKAGE_JSON}`)).version;
}

function extract(commit) {
  if (fs.existsSync(MANIFEST_PATH)) {
    const manifest = JSON.parse(fs.readFileSync(MANIFEST_PATH, "utf8"));
    if (manifest.commit === commit) {
      return;
    }
  }

  fs.rmSync(EXTRACT_ROOT, { recursive: true, force: true });
  fs.mkdirSync(EXTRACT_ROOT, { recursive: true });
  const archive = execFileSync(
    "git",
    [
      "archive",
      "--format=tar",
      commit,
      "--",
      `${CONVEX_PACKAGE_DIR}/src`,
      `${CONVEX_PACKAGE_DIR}/tsconfig.json`,
      // TypeDoc renders the README closest to the entry points' package.json.
      PACKAGE_JSON,
      `${CONVEX_PACKAGE_DIR}/README.md`,
    ],
    { cwd: REPO_ROOT, maxBuffer: 256 * 1024 * 1024 },
  );
  execFileSync("tar", ["-x", "-C", EXTRACT_ROOT], { input: archive });

  // The release resolves its dependencies from the working tree's
  // `node_modules`. They can be newer than the release's, which only matters
  // if a documented type or the CLI's behavior changed.
  fs.symlinkSync(
    path.join(REPO_ROOT, CONVEX_PACKAGE_DIR, "node_modules"),
    path.join(EXTRACT_ROOT, CONVEX_PACKAGE_DIR, "node_modules"),
    "dir",
  );

  fs.writeFileSync(MANIFEST_PATH, JSON.stringify({ commit }) + "\n");
}

function findRelease() {
  const version = JSON.parse(
    fs.readFileSync(path.join(REPO_ROOT, PACKAGE_JSON), "utf8"),
  ).version;
  if (!/^\d+\.\d+\.\d+$/.test(version)) {
    throw new Error(
      `${PACKAGE_JSON} is at ${version}, which isn't a stable release, so ` +
        `there's no release to document. Set ${SOURCE_ENV_VAR}=branch to ` +
        "document the working tree.",
    );
  }

  const tag = `npm/${version}`;
  if (ensureTag(tag)) {
    const commit = git("rev-parse", `${tag}^{commit}`);
    return {
      kind: "release",
      version,
      tag,
      commit,
      ...releaseSourceLinks(commit),
    };
  }

  const commit = lastVersionCommit();
  if (versionAt(commit) !== version) {
    throw new Error(
      `The last commit that changes the version in ${PACKAGE_JSON} ` +
        `(${commit}) sets it to ${versionAt(commit)}, not ${version}`,
    );
  }
  return {
    kind: "untagged",
    version,
    tag,
    commit,
    ...releaseSourceLinks(commit),
  };
}

/**
 * Returns the `convex` package that the generated reference pages document.
 *
 * - `root`: directory laid out like the repository root.
 * - `packageDir`: the `convex` package inside `root`.
 * - `linkRepo`, `linkRef`: the GitHub repository and ref that source links
 *   point at.
 * - `pageBanner`: Markdown to add to the top of every generated page, or "".
 */
function prepareReferenceSource() {
  const mode = process.env[SOURCE_ENV_VAR] || "branch";
  if (mode === "branch") {
    const commit = git("rev-parse", "HEAD");
    printBanner({ kind: "dev", head: commit, envVar: SOURCE_ENV_VAR });
    return {
      root: REPO_ROOT,
      packageDir: path.join(REPO_ROOT, CONVEX_PACKAGE_DIR),
      linkRepo: INTERNAL_REPO,
      linkRef: commit,
      pageBanner: BRANCH_BANNER,
    };
  }
  if (mode !== "release") {
    throw new Error(
      `${SOURCE_ENV_VAR} must be "branch" or "release", got "${mode}"`,
    );
  }

  const release = findRelease();
  extract(release.commit);
  printBanner(release);
  return {
    root: EXTRACT_ROOT,
    packageDir: path.join(EXTRACT_ROOT, CONVEX_PACKAGE_DIR),
    linkRepo: release.mirrorRepo,
    linkRef: release.mirrorRef,
    pageBanner: "",
  };
}

/**
 * Writes a tsconfig that compiles only `entryPoints` of `source`, and returns
 * its path.
 *
 * A release's `src` can fail to typecheck against the working tree's newer
 * dependencies in code the API reference doesn't document (e.g. the CLI).
 */
function writeEntryPointsTsconfig(source, entryPoints) {
  fs.mkdirSync(CACHE_DIR, { recursive: true });
  const tsconfigPath = path.join(CACHE_DIR, "tsconfig.api.json");
  fs.writeFileSync(
    tsconfigPath,
    JSON.stringify(
      {
        extends: path.join(source.packageDir, "tsconfig.json"),
        include: [],
        files: entryPoints,
      },
      null,
      2,
    ) + "\n",
  );
  return tsconfigPath;
}

module.exports = { prepareReferenceSource, writeEntryPointsTsconfig };
