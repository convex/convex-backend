import React, { useEffect, useState } from "react";

const AlgoliaAppID = "1KIE511890";
const defaultStackPostImage = "/img/stack-logo-light.svg";

interface StackResult {
  title: string;
  // This is the URL slug.
  objectID: string;
  summary: string;
  tags: string[];
  mainImageUrl?: string;
  authorName: string;
  authorImageUrl: string;
}
interface StackPostsProps {
  query: string;
}

export function StackPosts({ query }: StackPostsProps) {
  const [results, setResults] = useState<StackResult[]>();

  useEffect(() => {
    const performSearch = async () => {
      const queryObject = {
        query,
        filters: "type:article",
        hitsPerPage: 4,
      };
      try {
        const response = await fetch(
          `https://${AlgoliaAppID}.algolia.net/1/indexes/stack/query`,
          {
            method: "POST",
            headers: {
              "Content-Type": "application/json",
              // Search-only API key, safe to use in frontend code. See:
              // https://www.algolia.com/doc/guides/security/api-keys/#search-only-api-key
              "X-Algolia-API-Key": "07096f4c927e372785f8453f177afb16",
              "X-Algolia-Application-Id": AlgoliaAppID,
            },
            body: JSON.stringify(queryObject),
          },
        );
        if (response.ok) {
          const data = await response.json();
          setResults(data.hits);
        } else {
          console.error("Error during search", response);
        }
      } catch (error) {
        console.error("Error during search:", error);
      }
    };

    performSearch();
  }, [query]);

  return (
    <div className="StackPosts">
      <div className="StackPosts-title">
        Related posts from{" "}
        <a
          className="StackPosts-title-imageLink"
          href="https://stack.convex.dev/"
          target="_blank"
        >
          <img
            className="StackPosts-title-image StackPosts-title-image--dark"
            src="/img/stack-logo-dark.svg"
            width={96}
            height={24}
            alt="Stack"
          />
          <img
            className="StackPosts-title-image StackPosts-title-image--light"
            src="/img/stack-logo-light.svg"
            width={96}
            height={24}
            alt="Stack"
          />
        </a>
      </div>
      <div className="StackPosts-posts">
        {results?.map(
          ({ objectID, title, mainImageUrl, authorName, authorImageUrl }) => (
            <a
              key={objectID}
              className="StackPosts-post"
              href={`https://stack.convex.dev/${objectID}`}
              target="_blank"
            >
              <img
                className={`StackPosts-post-image${mainImageUrl ? "" : " StackPosts-post-image--default"}`}
                src={
                  mainImageUrl ? `${mainImageUrl}?h=188` : defaultStackPostImage
                }
                alt=""
              />
              <div className="StackPosts-post-content">
                <div className="StackPosts-post-content-title">{title}</div>
                <div className="StackPosts-post-content-author">
                  <img
                    className="StackPosts-post-content-author-image"
                    src={authorImageUrl}
                    width={24}
                    height={24}
                    alt={`Avatar of ${authorName}`}
                  />
                  <span className="StackPosts-post-content-author-name">
                    {authorName}
                  </span>
                </div>
              </div>
            </a>
          ),
        )}
      </div>
    </div>
  );
}
