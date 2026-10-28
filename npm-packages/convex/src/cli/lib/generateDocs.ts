import type { CommandUnknownOpts } from "@commander-js/extra-typings";

export type GeneratedDocs = Record<string, string>;

type CommandInfo = {
  command: CommandUnknownOpts;
  // Full path of command names from the root, e.g. ["convex", "env", "set"].
  path: string[];
};

/**
 * Generate Markdown reference docs for a Commander main command.
 *
 * Returns a map of file paths (relative, e.g. `env.mdx`) to Markdown
 * contents. One file is generated for each visible main command (a direct
 * child of the root); the root itself gets no page, so "Command Reference"
 * is a sidebar section with no landing page. Descendant subcommands are
 * rendered as sections inside their main command's file, with their heading
 * level reflecting their depth: a direct subcommand (`env default`) is an
 * `## h2`, a sub-subcommand (`env default set`) an `### h3`, and so on.
 *
 * Each page carries a `description` frontmatter equal to the command's
 * summary; the `/cli` page reads these back via Docusaurus metadata to render
 * the list of commands dynamically.
 */
export function generateDocs(root: CommandUnknownOpts): GeneratedDocs {
  const docs: GeneratedDocs = {};
  const all = collectCommands(root, [root.name()]);

  let mainCommandPosition = 0;
  for (const entry of all) {
    // Skip the root command (no landing page) and any descendant beyond a
    // main command (those are rendered inside their main command's file).
    if (entry.path.length !== 2) continue;
    const filePath = `${entry.path[1]}.mdx`;
    const sidebarPosition = ++mainCommandPosition;
    const summary =
      entry.command.summary() || entry.command.description() || "";
    // Collapse to a single line so it's a valid YAML frontmatter scalar, and
    // quote it safely (summaries contain `[BETA]`, `:`, etc.).
    const description = JSON.stringify(summary.replace(/\s+/g, " ").trim());

    const lines: string[] = [];
    const sidebarLabel = `npx ${entry.path.join(" ")}`;
    lines.push("---");
    lines.push(`sidebar_position: ${sidebarPosition}`);
    // Set an explicit `title` so the Docusaurus <title> (and browser tab) reads
    // the full command, e.g. `npx convex dev | …`. Without it Docusaurus
    // derives the title from the doc id (the file name), which would be just
    // `dev`.
    lines.push(`title: "${sidebarLabel}"`);
    lines.push(`sidebar_label: "${sidebarLabel}"`);
    lines.push(`description: ${description}`);
    lines.push("---");
    lines.push("");
    lines.push(
      "{/* @generated from the command definitions, do not edit manually (run `just regenerate-cli-docs` to regenerate) */}",
    );
    lines.push("");
    lines.push(renderCommand(entry, all, { headingLevel: 1 }));

    const descendants = all.filter(
      (e) =>
        e.path.length > entry.path.length &&
        entry.path.every((p, i) => e.path[i] === p),
    );
    for (const d of descendants) {
      // Render the heading hierarchically by depth below the root: a direct
      // subcommand (`env default`, path length 3) is an h2, a sub-subcommand
      // (`env default set`, path length 4) is an h3, and so on. This nests
      // sub-subcommands under their parent group instead of flattening every
      // descendant to the same h2 level.
      lines.push(
        renderCommand(d, all, {
          headingLevel: d.path.length - 1,
          includeSubcommandsList: false,
        }),
      );
    }

    docs[filePath] = lines.join("\n");
  }

  return docs;
}

function collectCommands(
  command: CommandUnknownOpts,
  path: string[],
): CommandInfo[] {
  const result: CommandInfo[] = [{ command, path }];
  for (const sub of command.commands) {
    // Skip hidden commands and Commander's built-in help command.
    if ((sub as any)._hidden) continue;
    if (sub.name() === "help") continue;
    result.push(...collectCommands(sub, [...path, sub.name()]));
  }
  return result;
}

function displayName(path: string[]): string {
  return `npx ${path.join(" ")}`;
}

// Build the usage suffix (everything after the command name) for the Syntax
// code block. We derive the argument portion from the command's registered
// arguments rather than trusting a manual `.usage()` override, so the Syntax
// line always agrees with the Arguments section below. For example `env set`
// overrides `.usage()` to show `<name> <value>` (pretending the args are
// required) for nicer `--help` output, even though it registers them as
// optional `[name] [value]`; reproducing the override verbatim would make the
// generated page contradict itself. Commands with no registered arguments keep
// their `.usage()` string verbatim (e.g. the root's `<command> [options]`).
function commandUsage(command: CommandUnknownOpts): string {
  const args = (command as any).registeredArguments ?? [];
  if (args.length === 0) {
    return command.usage();
  }
  // Mirror Commander's default usage order: options, then a subcommand
  // placeholder, then the positional arguments. `[options]` is always present
  // because the built-in `--help` option is always registered.
  const parts = ["[options]"];
  if (command.commands.length > 0) {
    parts.push("[command]");
  }
  for (const arg of args) {
    const name = `${arg._name}${arg.variadic ? "..." : ""}`;
    parts.push(arg.required ? `<${name}>` : `[${name}]`);
  }
  return parts.join(" ");
}

// Anchor id for a descendant command rendered as an h2 section inside its
// main command's file. Built from the path segments below the main command,
// joined with `-`, so sub-subcommands stay unique. For example, both
// `deployment create` and `deployment token create` would collide on `#create`
// if we used only the leaf name; instead they become `#create` and
// `#token-create`.
function anchorSlug(path: string[]): string {
  return path.slice(2).join("-");
}

// Link target for a subcommand listed inside a command's "Subcommands"
// section. From the root index, main commands live in sibling files. From a
// main command page, descendants are rendered as h2 sections in the same
// file, anchored by their path below the main command.
function subcommandLink(parentPath: string[], subPath: string[]): string {
  if (parentPath.length === 1) {
    return `./${subPath[1]}`;
  }
  return `#${anchorSlug(subPath)}`;
}

function renderCommand(
  entry: CommandInfo,
  all: CommandInfo[],
  options: { headingLevel: number; includeSubcommandsList?: boolean },
): string {
  const { headingLevel, includeSubcommandsList = true } = options;
  const h1 = "#".repeat(headingLevel);
  const h2 = "#".repeat(headingLevel + 1);

  const { command, path } = entry;
  const display = displayName(path);
  const description = renderCopyableCommands(
    escapeMdx(
      formatExampleLines(
        replaceBullets(command.description() || command.summary() || ""),
      ),
    ),
  );

  const lines: string[] = [];
  // For nested subcommand sections, pin the heading id to the path below the
  // main command so anchors like `#set` and `#token-create` stay unique even
  // when leaf names collide across different parents.
  const headingSuffix = headingLevel > 1 ? ` \\{#${anchorSlug(path)}}` : "";
  lines.push(`${h1} \`${display}\`${headingSuffix}`);
  lines.push("");
  if (description) {
    lines.push(description);
    lines.push("");
  }

  const usage = commandUsage(command);
  lines.push(`${h2} Syntax`);
  lines.push("");
  lines.push("```sh");
  lines.push(`${display} ${usage}`.trim());
  lines.push("```");
  lines.push("");

  const aliases = command.aliases();
  if (aliases.length > 0) {
    lines.push(`${h2} Aliases`);
    lines.push("");
    for (const alias of aliases) {
      lines.push(`- \`${alias}\``);
    }
    lines.push("");
  }

  const args = (command as any).registeredArguments ?? [];
  if (args.length > 0) {
    lines.push(`${h2} Arguments`);
    lines.push("");
    lines.push("<dl>");
    for (const arg of args) {
      const name = arg.required ? `<${arg._name}>` : `[${arg._name}]`;
      const desc = escapeMdx(replaceBullets(arg.description || ""));
      lines.push(`<dt>\`${name}\`</dt>`);
      lines.push(`<dd>`);
      lines.push("");
      lines.push(desc);
      lines.push("");
      lines.push(`</dd>`);
    }
    lines.push("</dl>");
    lines.push("");
  }

  const opts = command.options.filter((o: any) => !o.hidden);
  if (opts.length > 0) {
    lines.push(`${h2} Options`);
    lines.push("");
    lines.push("<dl>");
    for (const opt of opts) {
      const flags = (opt as any).flags as string;
      const desc = escapeMdx(replaceBullets(opt.description || ""));
      lines.push(`<dt>\`${flags}\`</dt>`);
      lines.push(`<dd>`);
      lines.push("");
      lines.push(desc);
      lines.push("");
      lines.push(`</dd>`);
    }
    lines.push("</dl>");
    lines.push("");
  }

  if (includeSubcommandsList) {
    const subEntries = all.filter(
      (e) =>
        e.path.length === path.length + 1 &&
        path.every((p, i) => e.path[i] === p),
    );
    if (subEntries.length > 0) {
      lines.push(`${h2} Subcommands`);
      lines.push("");
      for (const sub of subEntries) {
        const subDisplay = displayName(sub.path);
        const target = subcommandLink(path, sub.path);
        const subDesc = indentContinuation(
          escapeMdx(sub.command.summary() || sub.command.description() || ""),
        );
        lines.push(`- [\`${subDisplay}\`](${target}) — ${subDesc}`.trimEnd());
      }
      lines.push("");
    }
  }

  return lines.join("\n");
}

// Escape characters that MDX would otherwise parse as JSX. Backslash-escaping
// `<` and `{` keeps placeholder text like `<team_slug>` rendering as literal
// text instead of being interpreted as a tag or expression.
//
// MDX only parses `<` and `{` specially in prose — inside an inline code span
// (backticks) both are already literal, and a backslash there renders verbatim
// (e.g. `` `\<nameOrToken>` `` shows the backslash). So leave code spans
// untouched and escape only the surrounding prose.
function escapeMdx(text: string): string {
  return text
    .split(/(`[^`]*`)/)
    .map((segment, i) =>
      // Odd indices are the captured code spans; even indices are prose.
      i % 2 === 1 ? segment : segment.replace(/[<{]/g, (c) => `\\${c}`),
    )
    .join("");
}

// Turn inline code spans that are full `npx convex ...` commands into a
// CodeWithCopyButton component so readers can copy them with one click. The
// component is registered globally in the docs site's MDXComponents, so no
// import is needed in the generated page. This runs after escapeMdx, which
// leaves code spans verbatim, so the command text inside the backticks is
// unescaped and drops straight into the `text` attribute.
//
// We pass the command as a JSX expression containing a JSON-encoded string
// (`text={"..."}`) rather than a plain attribute (`text="..."`) because
// commands routinely contain double quotes (e.g. JSON arguments like
// `'{"body": "hello"}'`), which would otherwise terminate the attribute value
// and produce invalid MDX. JSON.stringify escapes quotes and backslashes for
// us.
function renderCopyableCommands(text: string): string {
  return text
    .split("\n")
    .map((line) =>
      /^- /.test(line)
        ? line.replace(
            /`(npx convex [^`]*)`/g,
            (_, command) =>
              `<CodeWithCopyButton text={${JSON.stringify(command)}} />`,
          )
        : line,
    )
    .join("\n");
}

// Replace "•" bullet points with markdown "-" bullet points. Every line that is
// (maybe leading spaces +) "• " becomes the same spaces + "- ". This runs before
// formatExampleLines and renderCopyableCommands so that `npx convex ...` commands
// in bulleted help text are recognized as list items and get a copy button.
export function replaceBullets(text: string): string {
  return text.replace(/^( *)• /gm, "$1- ");
}

// Convert indented example lines (e.g., from CLI help text) into markdown lists
// so they render with proper line breaks instead of collapsing into one line.
// Consecutive indented lines are gathered into a block; any line indented by at
// least two spaces belongs to the block, regardless of how deeply it nests.
//
// How much the block is dedented depends on what precedes it:
//   - When the block follows a list item, its indentation encodes real nesting
//     (a sub-list under that item), so it is preserved verbatim. This keeps a
//     3-level list like the deploy command's preview-key description intact
//     instead of flattening the 2-space sub-item to a top-level bullet.
//   - When the block follows prose (or starts the text), the indentation is
//     just help-text formatting, so the block is dedented by its smallest
//     indent to the left margin. Otherwise an indented list wouldn't interrupt
//     the preceding paragraph and would collapse into it.
function formatExampleLines(text: string): string {
  const lines = text.split("\n");
  const result: string[] = [];
  let blockLines: string[] = [];

  const flushBlock = () => {
    if (blockLines.length === 0) {
      return;
    }
    const prev = result[result.length - 1];
    const followsListItem = prev !== undefined && isListItem(prev);
    const dedent = followsListItem
      ? 0
      : Math.min(...blockLines.map(indentWidth));
    for (const blockLine of blockLines) {
      result.push(toListItem(blockLine.substring(dedent)));
    }
    blockLines = [];
  };

  for (const line of lines) {
    // Any line indented by at least two spaces (and not blank) is part of an
    // example block, regardless of nesting depth.
    if (/^ {2,}\S/.test(line)) {
      blockLines.push(line);
    } else {
      flushBlock();
      result.push(line);
    }
  }
  flushBlock();

  return result.join("\n");
}

function indentWidth(line: string): number {
  return line.length - line.trimStart().length;
}

function isListItem(line: string): boolean {
  const content = line.trimStart();
  return content.startsWith("- ") || /^\d+\.\s/.test(content);
}

// Render an example line as a markdown list item, preserving any leading indent
// so nested items stay nested. Lines that are already list items — either a
// bullet (`- `) or an ordered-list item (`1. `) — are kept verbatim so they
// render as their intended list type. Prefixing an ordered-list item with `- `
// would nest it inside a bullet, which renders as roman numerals instead of the
// numbered steps.
function toListItem(line: string): string {
  if (isListItem(line)) {
    return line;
  }
  const indent = line.slice(0, indentWidth(line));
  return `${indent}- ${line.trimStart()}`;
}

function indentContinuation(text: string): string {
  return text.replace(/\n/g, "\n  ");
}
