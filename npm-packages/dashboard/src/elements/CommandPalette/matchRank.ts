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
// `isWordStart` can see camelCase boundaries.
function fold(text: string): string {
  return text
    .normalize("NFD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[\s_-]+/g, " ")
    .trim();
}

// The form both the filter and the ranker compare: folded and lowercased.
export function normalizeForSearch(text: string): string {
  return fold(text).toLowerCase();
}

// Case-based checks rather than `\p{…}` classes, which need an ES2015 target.
const isUpper = (char: string) => char !== char.toLowerCase();
const isLower = (char: string) => char !== char.toUpperCase();
const isDigit = (char: string) => char >= "0" && char <= "9";

// Whether a word starts at `index`: after a non-alphanumeric character, or at a
// camelCase hump.
function isWordStart(text: string, index: number): boolean {
  if (index === 0) {
    return true;
  }
  const previous = text[index - 1];
  if (!isUpper(previous) && !isLower(previous) && !isDigit(previous)) {
    return true;
  }
  return (isLower(previous) || isDigit(previous)) && isUpper(text[index]);
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
