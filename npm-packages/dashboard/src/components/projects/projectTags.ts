import { SLUG_CHARSET } from "lib/slugs";

// Mirrors `validate_tags` in Big Brain's `project_tags.rs`, so a tag the input
// accepts is one the server accepts too.
export const MAX_PROJECT_TAGS = 5;
export const MAX_PROJECT_TAG_LENGTH = 32;

/**
 * Why `tag` can't join `currentTags` (a project's tags, or a filter's), or
 * undefined if it can. `tag` should already be trimmed.
 */
export function validateProjectTag(
  tag: string,
  currentTags: string[],
): string | undefined {
  if (currentTags.length >= MAX_PROJECT_TAGS) {
    return `You can add at most ${MAX_PROJECT_TAGS} tags.`;
  }
  // Big Brain counts Unicode scalar values, which `Array.from` iterates.
  if (Array.from(tag).length > MAX_PROJECT_TAG_LENGTH) {
    return `Tags can be at most ${MAX_PROJECT_TAG_LENGTH} characters long.`;
  }
  if (!SLUG_CHARSET.test(tag)) {
    return "Tags can contain only letters, numbers, '-', and '_'.";
  }
  const lowerTag = tag.toLowerCase();
  if (currentTags.some((t) => t.toLowerCase() === lowerTag)) {
    return "This tag is already added.";
  }
  return undefined;
}
