// How well a search matches a candidate string. Lower tiers rank first.
export const MatchTier = {
  // "convex" → "Convex"
  Exact: 0,
  // "convex" → "Convex Playground"
  Prefix: 1,
  // "convex" → "Nicolas Convex", "myConvexApp"
  WordPrefix: 2,
  // "convex" → "myconvexapp"
  Substring: 3,
  // "playground convex" → "Convex Playground": every token appears, but the
  // search as a whole doesn't.
  AllTokens: 4,
} as const;
export type MatchTier = (typeof MatchTier)[keyof typeof MatchTier];

export type MatchRank = {
  tier: MatchTier;
  // Whether the candidate contains the search exactly as typed, separators and
  // accents included. Within a tier, "convex-play" ranks `convex-playground`
  // above "Convex Playground", and "convex play" the other way around.
  asTyped: boolean;
  // Length of the best-matching candidate: within a tier, the candidate closest
  // in length to the search is the closest match.
  length: number;
};

// Strips accents and treats runs of whitespace, `-` and `_` as a single space,
// so "convex playground" matches `convex-playground`. Case is kept so that
// `isWordStart` can see camelCase boundaries. `sources[i]` is the index in
// `text` that `folded[i]` came from, so a match in the folded text can be
// highlighted in the original.
function foldWithSources(text: string): { folded: string; sources: number[] } {
  let folded = "";
  const sources: number[] = [];
  // Walks UTF-16 code units, not code points: a character outside the BMP comes
  // through as its two surrogates, each folding to itself, so `sources` stays
  // aligned with `folded` one unit at a time.
  for (let index = 0; index < text.length; index++) {
    const char = text[index];
    if (/[\s_-]/.test(char)) {
      if (!folded.endsWith(" ")) {
        folded += " ";
        sources.push(index);
      }
      continue;
    }
    // One character can decompose into several ("é" → "e" + accent), of which
    // only the accent is dropped.
    for (const part of char.normalize("NFD").replace(/[\u0300-\u036f]/g, "")) {
      folded += part;
      sources.push(index);
    }
  }
  return { folded, sources };
}

function fold(text: string): string {
  return foldWithSources(text).folded.trim();
}

// The form both the filter and the ranker compare: folded and lowercased.
export function normalizeForSearch(text: string): string {
  return fold(text).toLowerCase();
}

// Case-based checks rather than `\p{…}` classes, which need an ES2015 target.
const isUpper = (char: string) => char !== char.toLowerCase();
const isLower = (char: string) => char !== char.toUpperCase();
const isDigit = (char: string) => char >= "0" && char <= "9";

// Whether a word starts at `index`: after a non-alphanumeric character, at a
// camelCase hump ("my|Convex"), or where an acronym ends and a capitalized
// word begins ("API|Key").
function isWordStart(text: string, index: number): boolean {
  if (index === 0) {
    return true;
  }
  const previous = text[index - 1];
  const char = text[index];
  if (!isUpper(previous) && !isLower(previous) && !isDigit(previous)) {
    return true;
  }
  if ((isLower(previous) || isDigit(previous)) && isUpper(char)) {
    return true;
  }
  const next = text[index + 1];
  return (
    isUpper(previous) && isUpper(char) && next !== undefined && isLower(next)
  );
}

function matchTier(
  needle: string,
  tokens: string[],
  cased: string,
  haystack: string,
): MatchTier | null {
  if (haystack === needle) {
    return MatchTier.Exact;
  }
  if (haystack.startsWith(needle)) {
    return MatchTier.Prefix;
  }
  let index = haystack.indexOf(needle);
  if (index !== -1) {
    while (index !== -1) {
      if (isWordStart(cased, index)) {
        return MatchTier.WordPrefix;
      }
      index = haystack.indexOf(needle, index + 1);
    }
    return MatchTier.Substring;
  }
  if (tokens.length > 1 && tokens.every((token) => haystack.includes(token))) {
    return MatchTier.AllTokens;
  }
  return null;
}

function rankCandidate(
  typed: string,
  needle: string,
  tokens: string[],
  candidate: string,
): MatchRank | null {
  const cased = fold(candidate);
  const haystack = cased.toLowerCase();
  const tier = matchTier(needle, tokens, cased, haystack);
  if (tier === null) {
    return null;
  }
  return {
    tier,
    asTyped: candidate.toLowerCase().includes(typed),
    length: haystack.length,
  };
}

export function compareMatchRanks(a: MatchRank, b: MatchRank): number {
  return (
    a.tier - b.tier ||
    Number(b.asTyped) - Number(a.asTyped) ||
    a.length - b.length
  );
}

// The best rank of `search` across an item's candidate strings (e.g. its name
// and its slug), or null if none of them match.
export function rankMatch(
  search: string,
  candidates: readonly (string | null | undefined)[],
): MatchRank | null {
  const needle = normalizeForSearch(search);
  if (!needle) {
    return null;
  }
  const typed = search.trim().toLowerCase();
  const tokens = needle.split(" ");
  let best: MatchRank | null = null;
  for (const candidate of candidates) {
    if (!candidate) {
      continue;
    }
    const rank = rankCandidate(typed, needle, tokens, candidate);
    if (rank && (!best || compareMatchRanks(rank, best) < 0)) {
      best = rank;
    }
  }
  return best;
}

// Orders `items` by how well `search` matches them, best first. The sort is
// stable, so ties and an empty search keep the incoming order. Items that don't
// match go last: cmdk's own filter may still keep them through keywords that
// aren't worth ranking on (e.g. "component").
export function sortByMatch<T>(
  items: readonly T[],
  search: string,
  candidates: (item: T) => readonly (string | null | undefined)[],
): T[] {
  if (!fold(search)) {
    return [...items];
  }
  return items
    .map((item) => ({ item, rank: rankMatch(search, candidates(item)) }))
    .sort((a, b) => {
      if (a.rank && b.rank) {
        return compareMatchRanks(a.rank, b.rank);
      }
      return (a.rank ? 0 : 1) - (b.rank ? 0 : 1);
    })
    .map(({ item }) => item);
}

// `sortByMatch` for results the server returns a page at a time: each page is
// ranked on its own and the pages stay in order. Ranking across pages would
// place a better match from a page loaded on scroll above the results the user
// has already scrolled past.
export function sortPagesByMatch<T>(
  pages: readonly (readonly T[])[],
  search: string,
  candidates: (item: T) => readonly (string | null | undefined)[],
): T[] {
  return pages.flatMap((page) => sortByMatch(page, search, candidates));
}

// The indices of `text` to highlight for `search`: every occurrence of each of
// its tokens, compared the way the filter compares them (see
// normalizeForSearch). Where the whole search matches as one phrase, the
// separators inside it are highlighted too, so "convex playground" highlights
// all of `convex-playground` rather than two words with a gap. A search made
// only of separators matches as typed, as in the filter.
export function matchedIndices(search: string, text: string): Set<number> {
  const indices = new Set<number>();
  const needle = normalizeForSearch(search);
  if (!needle) {
    const typed = search.trim().toLowerCase();
    const lower = text.toLowerCase();
    let index = typed ? lower.indexOf(typed) : -1;
    while (index !== -1) {
      for (let offset = 0; offset < typed.length; offset++) {
        indices.add(index + offset);
      }
      index = lower.indexOf(typed, index + 1);
    }
    return indices;
  }
  const phrases = [needle, ...needle.split(" ")];
  const { folded, sources } = foldWithSources(text);
  const haystack = folded.toLowerCase();
  // Lowercasing changed the length (rare, e.g. some ligatures), so folded
  // positions no longer line up with `sources`.
  if (haystack.length !== folded.length) {
    return indices;
  }
  const highlighted = new Array<boolean>(haystack.length).fill(false);
  for (const phrase of phrases) {
    let index = haystack.indexOf(phrase);
    while (index !== -1) {
      highlighted.fill(true, index, index + phrase.length);
      index = haystack.indexOf(phrase, index + 1);
    }
  }
  highlighted.forEach((isHighlighted, index) => {
    if (!isHighlighted) {
      return;
    }
    // A folded character stands for everything up to the next one's source: a
    // whole run of separators, or a letter and the accents stripped from it.
    const end = Math.max(sources[index] + 1, sources[index + 1] ?? text.length);
    for (let source = sources[index]; source < end; source++) {
      indices.add(source);
    }
  });
  return indices;
}
