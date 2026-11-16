import { DotsHorizontalIcon } from "@radix-ui/react-icons";
import algoliasearch from "algoliasearch/lite";
import React, { useEffect, useState } from "react";
import ResultList from "./ResultList";
import { AlgoliaResult, Result } from "./types";

// Search-only API key, safe to use in frontend code. See:
// https://www.algolia.com/doc/guides/security/api-keys/#search-only-api-key
const searchClient = algoliasearch(
  "1KIE511890",
  "07096f4c927e372785f8453f177afb16",
);

const SEARCH_INDEXES = ["docs", "stack", "discord"] as const;
const DISCORD_GUILD_ID = "1019350475847499849";

interface ResultsProps {
  query: string;
}

export default function Results({ query }: ResultsProps) {
  const [results, setResults] = useState<Result[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState(false);

  useEffect(() => {
    let active = true;
    const trimmedQuery = query.trim();
    setResults([]);
    setError(false);
    setLoading(trimmedQuery !== "");

    if (trimmedQuery === "") {
      return;
    }

    void searchClient
      .search<AlgoliaResult>(
        SEARCH_INDEXES.map((indexName) => ({
          indexName,
          query: trimmedQuery,
          params: {
            hitsPerPage: 10,
            attributesToRetrieve: ["title", "objectID"],
            attributesToHighlight: [],
            attributesToSnippet: [],
          },
        })),
      )
      .then((response) => {
        if (!active) {
          return;
        }

        const combinedResults = response.results.flatMap((result, index) => {
          if (!("hits" in result)) {
            return [];
          }

          const indexName = SEARCH_INDEXES[index];
          return result.hits.map((hit) => {
            // Legacy Discord identifiers include a channel prefix; their last
            // segment is the thread ID. Docs use full URLs and Stack uses slugs.
            const url =
              indexName === "docs"
                ? toRelativeUrl(hit.objectID)
                : indexName === "stack"
                  ? `https://stack.convex.dev/${hit.objectID}`
                  : `https://discord.com/channels/${DISCORD_GUILD_ID}/${hit.objectID.split("/").pop()}`;

            return { title: hit.title || "Convex Docs", url };
          });
        });

        const seenUrls = new Set<string>();
        setResults(
          combinedResults.filter(({ url }) => {
            if (seenUrls.has(url)) return false;
            seenUrls.add(url);
            return true;
          }),
        );
      })
      .catch((error) => {
        if (active) {
          console.warn("Search request failed:", error);
          setError(true);
        }
      })
      .finally(() => {
        if (active) {
          setLoading(false);
        }
      });

    // A slower response for an earlier query must not replace newer results.
    return () => {
      active = false;
    };
  }, [query]);

  return (
    <div className="flex flex-col overflow-hidden gap-3 grow">
      <div
        className="overflow-y-auto grow flex flex-col gap-2"
        // Keeps the scroll bar light in dark mode.
        style={{
          colorScheme: "light",
        }}
      >
        <ResultList results={results} />
        {error && (
          <p role="status" className="px-2 text-neutral-n9">
            Search is unavailable. Try another search or use Ask AI above.
          </p>
        )}
        {loading && (
          <div className="flex justify-center items-center">
            <DotsHorizontalIcon className="w-8 h-8 text-neutral-n8 animate-pulse" />
          </div>
        )}
      </div>
    </div>
  );
}

function toRelativeUrl(url: string): string {
  try {
    const parsed = new URL(url);
    if (parsed.hostname === "docs.convex.dev") {
      return parsed.pathname + parsed.search + parsed.hash;
    }
  } catch {
    // Relative links already point to the current deployment.
  }
  return url;
}
