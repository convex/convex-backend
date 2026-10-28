import React, { ReactNode } from "react";
import Link from "@docusaurus/Link";
import { useDocById } from "@docusaurus/plugin-content-docs/client";

type Item = {
  docId: string;
  href: string;
  label: string;
  icon?: ReactNode;
  invertIcon?: true;
};

export function DocCardList(props: { items: Item[] }) {
  const { items } = props;
  return (
    <ul className="cards">
      {items.map((item, index) => (
        <li key={index}>
          <CardLink item={item} />
        </li>
      ))}
    </ul>
  );
}

export function CardLink({
  className,
  item,
}: {
  className?: string;
  item: Item;
}) {
  const doc = useDocById(item.docId ?? undefined);
  const icon = item.icon;
  return (
    <Link
      href={item.href}
      className={
        "card" +
        (item.invertIcon ? " convex-invert-icon" : "") +
        " " +
        (className ?? "")
      }
    >
      {icon}
      <div>
        <div className="card__title text--truncate" title={item.label}>
          {item.label}
        </div>
        <p className="text--truncate" title={doc?.description}>
          {doc?.description}
        </p>
      </div>
    </Link>
  );
}
