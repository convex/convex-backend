import React from "react";
import CopyButton from "./CopyButton";

export function CodeWithCopyButton({ text }: { text: string }) {
  return (
    <code className="convex-inline-code-with-copy-button">
      {text}
      <span className="convex-inline-code-copy-button">
        <CopyButton code={text} />
      </span>
    </code>
  );
}
