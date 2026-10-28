import React from "react";
import CodeBlock from "./theme/CodeBlock/CodeBlock";

// Otherwise file extension === language name
const FILE_EXTENSION_TO_LANGUAGE = {
  rs: "rust",
  jsonl: "json",
};

/**
 * Create a snippet from a given TSX program `source`.
 *
 * Snippet lines can either be
 *
 *   // @snippet start <name>
 *
 * or
 *
 *   // @snippet end <name>
 *
 * which start and end the named snippet, respectively. These snippets name a
 * span in the original source file.
 *
 * If `snippet` is not provided, this function returns the source file with all
 * snippet comment lines removed. Otherwise, it extracts the selected snippet.
 *
 * Other special comments:
 * // @skipNextLine
 */
export function Snippet({
  title,
  titleSuffix,
  link,
  source,
  snippet,
  prefix,
  suffix,
  highlightPatterns,
  replacements = [],
}: {
  source: string;
  title?: string;
  titleSuffix?: string;
  link?: string;
  snippet?: string | string[];
  prefix?: string;
  suffix?: string;
  highlightPatterns?: (string | { from: string; to: string })[];
  replacements?: [RegExp | string, string][];
}) {
  const lines = source.split("\n");
  const allSpans = new Map<string, { start: number; end: number }>();
  const openSpans = new Map<string, { start: number }>();
  const nonSnippetLines = [];
  const linesOmittingSkipped = [];
  let lineIndex = 0;
  while (lineIndex < lines.length) {
    const line = lines[lineIndex];
    const trimmed = line.trim();
    if (trimmed.startsWith("// @skipNextLine")) {
      lineIndex += 2;
      continue;
    } else {
      linesOmittingSkipped.push(line);
      lineIndex += 1;
    }
  }
  for (const line of linesOmittingSkipped) {
    const trimmed = line.trim();
    if (
      trimmed.startsWith("// @snippet") ||
      trimmed.startsWith("{/* @snippet")
    ) {
      const pieces = trimmed.split(" ");
      if (pieces.length < 4) {
        throw new Error(`Invalid snippet line: ${trimmed}`);
      }
      const [_comment, _snippet, command, name] = pieces;
      if (command === "start") {
        if (allSpans.has(name) || openSpans.has(name)) {
          throw new Error(`Duplicate span ${name}`);
        }
        openSpans.set(name, { start: nonSnippetLines.length });
      } else if (command === "end") {
        if (!openSpans.has(name)) {
          throw new Error(`Invalid span close ${name}`);
        }
        const openSpan = openSpans.get(name);
        openSpans.delete(name);
        const span = { start: openSpan.start, end: nonSnippetLines.length };
        allSpans.set(name, span);
      } else {
        throw new Error(`Invalid snippet command in ${trimmed}`);
      }
    } else {
      nonSnippetLines.push(line);
    }
  }
  if (openSpans.size > 0) {
    throw new Error(`Spans left open: ${Array.from(openSpans.keys())}`);
  }
  let finalSource;
  if (snippet) {
    const spans: [string, { start: number; end: number }][] = (
      Array.isArray(snippet) ? snippet : [snippet]
    ).map((snippet) => [snippet, allSpans.get(snippet)]);
    const [invalidSnippet] =
      spans.find(([_snippet, span]) => span === undefined) ?? [];
    if (invalidSnippet !== undefined) {
      throw new Error(`Invalid snippet="${invalidSnippet}"`);
    }
    console.log(
      ...spans.map(([_, span]) => nonSnippetLines.slice(span.start, span.end)),
    );
    finalSource = trimLeftWhitespace(
      (prefix?.split("\n") ?? []).concat(
        ...spans.map(([_, span]) =>
          nonSnippetLines.slice(span.start, span.end),
        ),
        suffix?.split("\n") ?? [],
      ),
    ).join("\n");
  } else {
    finalSource = nonSnippetLines.join("\n");
  }
  for (const [pat, replace] of replacements) {
    finalSource = finalSource.replaceAll(pat, replace);
  }
  finalSource = highlightPatterns
    ? highlightLines(finalSource, highlightPatterns)
    : finalSource;
  const fileExtension = title?.match(/\.([^.]+)$/)?.[1];
  const language =
    FILE_EXTENSION_TO_LANGUAGE[fileExtension] ?? fileExtension ?? "tsx";

  const combinedTitle =
    title === undefined ? undefined : (
      <>
        {link ? (
          <a href={link} target="_blank">
            {title}
          </a>
        ) : (
          title
        )}
        {titleSuffix}
      </>
    );

  return (
    <CodeBlock
      className={"language-" + language}
      // CodeBlock is wrongly typed
      title={combinedTitle as unknown as string}
    >
      {finalSource}
    </CodeBlock>
  );
}

function trimLeftWhitespace(lines: string[]): string[] {
  const nonEmptyLines = lines.filter((line) => line.trim().length > 0);
  const toTrim = Math.min(
    ...nonEmptyLines.map((line) => line.length - line.trimStart().length),
  );
  return lines.map((line) => (line !== "" ? line.slice(toTrim) : ""));
}

/**
 * Insert Docusaurus `// highlight-next-line` markers above lines selected by
 * `patterns`. A string pattern highlights every line it matches. A
 * `{ from, to }` pattern highlights the inclusive block from the first line
 * matching `from` to the first line at or after it matching `to`.
 *
 * Throws if any pattern matches no lines, so a stale or mistyped pattern fails
 * the Docusaurus build instead of silently highlighting nothing.
 */
function highlightLines(
  source: string,
  patterns: (string | { from: string; to: string })[],
): string {
  const linesToHighlight = new Set<number>();
  const lines = source.split("\n");
  for (const pattern of patterns) {
    if (typeof pattern === "string") {
      const matchedIndices = lines
        .map((line, i) => (line.match(pattern) ? i : -1))
        .filter((i) => i !== -1);
      if (matchedIndices.length === 0) {
        throw new Error(
          `Snippet highlightPatterns: no line matched ${JSON.stringify(pattern)}`,
        );
      }
      for (const i of matchedIndices) {
        linesToHighlight.add(i);
      }
    } else {
      const { from, to } = pattern;
      const startIndex = lines.findIndex((line) => line.match(from));
      if (startIndex === -1) {
        throw new Error(
          `Snippet highlightPatterns: no line matched from=${JSON.stringify(from)}`,
        );
      }
      const endOffset = lines
        .slice(startIndex)
        .findIndex((line) => line.match(to));
      if (endOffset === -1) {
        throw new Error(
          `Snippet highlightPatterns: no line matched to=${JSON.stringify(to)} after from=${JSON.stringify(from)}`,
        );
      }
      const endIndex = startIndex + endOffset;
      const range = Array.from(
        { length: endIndex - startIndex + 1 },
        (_, k) => startIndex + k,
      );
      for (const i of range) {
        linesToHighlight.add(i);
      }
    }
  }
  for (const i of Array.from(linesToHighlight).sort((a, b) => b - a)) {
    lines.splice(i, 0, "// highlight-next-line");
  }
  return lines.join("\n");
}
