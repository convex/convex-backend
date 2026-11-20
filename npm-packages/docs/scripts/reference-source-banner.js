// Formats the build-log banner that says which `convex` commit the generated
// reference pages document. The colors and layout follow the CLI's deployment
// banner (npm-packages/convex/src/cli/lib/announceDeploymentTarget.ts).

const STYLES = {
  release: {
    label: "Release",
    emoji: "📦",
    bg: "#eacae7",
    fg: "#641aa9",
    bar: "#8b21f1",
  },
  untagged: {
    label: "Release (untagged)",
    emoji: "🚧",
    bg: "#fcedd7",
    fg: "#933615",
    bar: "#e25706",
  },
  dev: {
    label: "Dev build",
    emoji: "🔧",
    bg: "#d2ecbb",
    fg: "#2b6536",
    bar: "#3ea34b",
  },
};

const short = (sha) => sha.slice(0, 10);

/**
 * `source` is one of:
 * - `{ kind: "release", version, tag, commit, mirrorRepo, mirrorRef }`
 * - `{ kind: "untagged", version, tag, commit }`
 * - `{ kind: "dev", head, envVar }`
 *
 * `hyperlinks` renders links as OSC 8 hyperlinks, which only terminals
 * display; elsewhere each link is followed by its URL in parentheses.
 */
function formatReferenceSourceBanner(source, { chalk, hyperlinks }) {
  const color = chalk.level > 0;
  const link = (text, url) =>
    hyperlinks
      ? `\x1b]8;;${url}\x1b\\${chalk.underline(text)}\x1b]8;;\x1b\\`
      : `${text} (${url})`;
  const commitLink = (repo, sha) =>
    link(
      sha === "main" ? sha : short(sha),
      sha === "main"
        ? `https://github.com/${repo}/tree/main`
        : `https://github.com/${repo}/commit/${sha}`,
    );

  const style = STYLES[source.kind];
  const bar = chalk.hex(style.bar)("▌");
  const tag = color
    ? chalk.bold.bgHex(style.bg).hex(style.fg)(
        ` ${style.emoji} ${style.label} `,
      )
    : `[${style.emoji} ${style.label}]`;

  let main;
  let detail;
  switch (source.kind) {
    case "release":
      main =
        chalk.bold(
          link(
            `convex@${source.version}`,
            `https://www.npmjs.com/package/convex/v/${source.version}`,
          ),
        ) +
        " from " +
        chalk.bold(
          link(
            source.tag,
            `https://github.com/convex/convex/tree/${source.tag}`,
          ),
        );
      detail =
        `${chalk.dim("commit")} ${commitLink("convex/convex", source.commit)} ` +
        `${chalk.dim("· source links →")} ${chalk.dim(`${source.mirrorRepo}@`)}` +
        commitLink(source.mirrorRepo, source.mirrorRef);
      break;
    case "untagged":
      main =
        `${chalk.bold(`convex@${source.version}`)} from ` +
        chalk.bold(commitLink("convex/convex", source.commit)) +
        chalk.dim(", the commit that set the version");
      detail =
        `⚠️  No ${source.tag} tag yet. ` +
        chalk.dim("Expected while releasing; rebuild once the tag is pushed.");
      break;
    case "dev":
      main =
        `Using the source of the ${chalk.bold("convex")} package in your ` +
        `checkout ${chalk.dim(`(HEAD ${short(source.head)} + any uncommitted changes)`)}`;
      detail =
        chalk.dim(
          "To see what docs.convex.dev shows (the latest release), set ",
        ) +
        chalk.bold(`${source.envVar}=release`) +
        chalk.dim(".");
      break;
    default:
      throw new Error(`Unknown reference source kind: ${source.kind}`);
  }

  return [
    `${bar} 📚 ${chalk.bold("Reference docs")} ${chalk.dim("(API + CLI)")}`,
    `${bar} ${tag} ${main}`,
    `${bar} ${chalk.dim("└─")} ${detail}`,
  ].join("\n");
}

module.exports = { formatReferenceSourceBanner };
