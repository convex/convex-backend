/**
 * Highlights substrings of `text` that match `query` (case-insensitive).
 * Returns the original text unchanged when query is empty.
 */
export function HighlightMatch({
  text,
  query,
}: {
  text: string;
  query?: string;
}) {
  if (!query || query.trim() === "") {
    return <>{text}</>;
  }

  const escaped = query.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const parts = text.split(new RegExp(`(${escaped})`, "gi"));

  return (
    <>
      {parts.map((part, i) =>
        part.toLowerCase() === query.toLowerCase() ? (
          <span key={i} className="font-semibold text-content-accent">
            {part}
          </span>
        ) : (
          part
        ),
      )}
    </>
  );
}
