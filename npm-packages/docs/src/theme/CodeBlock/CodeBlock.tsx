import CodeBlock from "@theme-original/CodeBlock";
import React, { ReactNode } from "react";

export default function CodeBlockWrapper({
  metastring,
  title: titleProp,
  ...props
}: {
  className?: string;
  metastring?: string;
  originalType?: string;
  title?: ReactNode;
  children?: ReactNode;
}) {
  const title = parseCodeBlockTitle(metastring) ?? titleProp;

  return <CodeBlock title={title as unknown as string} {...props} />;
}

export function parseCodeBlockTitle(metastring?: string) {
  return metastring?.match(codeBlockTitleRegex)?.groups!.title;
}

const codeBlockTitleRegex = /title=(?<quote>["'])(?<title>.*?)\1/;
