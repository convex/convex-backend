import React from "react";
import Link from "@docusaurus/Link";
import {
  useDocsSidebar,
  findSidebarCategory,
  useDocById,
} from "@docusaurus/plugin-content-docs/client";
import type { PropSidebarItemLink } from "@docusaurus/plugin-content-docs";

// Renders the list of all "Command Reference" subcommands by reading the
// sidebar tree and per-doc frontmatter from Docusaurus metadata.
export default function CliCommandList(): JSX.Element {
  const sidebar = useDocsSidebar();
  const category = sidebar
    ? findSidebarCategory(sidebar.items, (c) => c.label === "Command Reference")
    : undefined;
  const links = (category?.items ?? []).filter(
    (item): item is PropSidebarItemLink => item.type === "link",
  );
  return (
    <ul>
      {links.map((item) => (
        <CommandListItem key={item.href} item={item} />
      ))}
    </ul>
  );
}

function CommandListItem({ item }: { item: PropSidebarItemLink }) {
  const doc = useDocById(item.docId ?? undefined);
  return (
    <li>
      <Link to={item.href}>
        <code>{item.label}</code>
      </Link>
      {doc?.description ? <> — {doc.description}</> : null}
    </li>
  );
}
