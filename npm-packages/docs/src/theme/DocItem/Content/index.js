import React from "react";
import DocItemContent from "@theme-original/DocItem/Content";

export default function DocItemContentWrapper({ children }) {
  return (
    <DocItemContent>
      <blockquote className="sr-only" inert="inert">
        For AI agents: see <a href="/llms.txt">llms.txt</a> for the complete
        documentation index. Markdown versions are available by adding .md to a
        page URL or requesting Accept: text/markdown.
      </blockquote>
      {children}
    </DocItemContent>
  );
}
