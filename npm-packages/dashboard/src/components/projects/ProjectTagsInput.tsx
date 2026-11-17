import { ChipInput } from "@ui/ChipInput";
import { ReactNode, useState } from "react";
import { MAX_PROJECT_TAGS, validateProjectTag } from "./projectTags";

/**
 * A list of project tags, each a removable pill, plus a text input that adds
 * the typed text as a tag once it passes the rules Big Brain checks tags
 * against, or offers matching `suggestions` to pick from.
 */
export function ProjectTagsInput({
  tags,
  onChange,
  suggestions = [],
  label = "Tags",
  labelHidden = false,
  placeholder = "Add tags",
  description,
  disabled = false,
}: {
  tags: string[];
  onChange: (tags: string[]) => void;
  /** Tags already in use, e.g. on the team's other projects. */
  suggestions?: string[];
  label?: string;
  labelHidden?: boolean;
  placeholder?: string;
  /** Shown beneath the input while there's no error to show instead. */
  description?: ReactNode;
  disabled?: boolean;
}) {
  const [query, setQuery] = useState("");
  const trimmedQuery = query.trim();
  const lowerQuery = trimmedQuery.toLowerCase();

  const error =
    trimmedQuery === "" ? undefined : validateProjectTag(trimmedQuery, tags);
  // Big Brain compares tags case-insensitively, so a suggestion that differs
  // from an added tag or the query only in case is the same tag.
  const lowerTags = new Set(tags.map((tag) => tag.toLowerCase()));
  const matchingSuggestions =
    tags.length >= MAX_PROJECT_TAGS
      ? []
      : suggestions.filter(
          (suggestion) =>
            !lowerTags.has(suggestion.toLowerCase()) &&
            suggestion.toLowerCase().includes(lowerQuery),
        );
  const exactSuggestion =
    trimmedQuery === ""
      ? undefined
      : matchingSuggestions.find(
          (suggestion) => suggestion.toLowerCase() === lowerQuery,
        );
  // The tag Enter or blur adds: an existing tag keeps its spelling, so the
  // query "Prod" reuses the team's "prod".
  const typedTag =
    trimmedQuery !== "" && error === undefined
      ? (exactSuggestion ?? trimmedQuery)
      : undefined;
  const newTag = exactSuggestion === undefined ? typedTag : undefined;

  const setTags = (newTags: string[]) => {
    onChange(newTags);
    setQuery("");
  };
  const addTypedTag = () => {
    if (typedTag !== undefined) {
      setTags([...tags, typedTag]);
    }
  };

  return (
    <ChipInput
      label={label}
      labelHidden={labelHidden}
      values={tags}
      onChange={setTags}
      chipLabel={(tag) => tag}
      removeLabel={(tag) => `Remove tag ${tag}`}
      removeTip="Remove tag"
      query={query}
      onQueryChange={setQuery}
      options={[
        ...matchingSuggestions.map((suggestion) => ({
          value: suggestion,
          label: suggestion,
        })),
        ...(newTag === undefined
          ? []
          : [{ value: newTag, label: `Add tag "${newTag}"` }]),
      ]}
      onEnterQuery={addTypedTag}
      // Keep a typed tag the user didn't confirm with Enter, so clicking Save
      // right after typing doesn't drop it.
      onBlur={addTypedTag}
      placeholder={placeholder}
      disabled={disabled}
      error={error}
      description={description}
    />
  );
}
