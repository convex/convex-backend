import { ReactNode } from "react";
import Link from "next/link";
import classNames from "classnames";
import { UrlObject } from "url";
import { DotsVerticalIcon } from "@radix-ui/react-icons";
import { Menu, MenuItem } from "@ui/Menu";

export type CardProps = {
  cardClassName?: string;
  contentClassName?: string;
  href?: string | UrlObject;
  children: ReactNode;
  dropdownItems?: {
    Icon: React.FC<{ className: string | undefined }>;
    text: string;
    action: () => void;
    destructive?: boolean;
    disabled?: boolean;
    tip?: ReactNode;
  }[];
  // Will be rendered as sibling to Card content
  overlayed?: ReactNode;
  // Accessible label for the card's overlay link (used as sr-only text)
  linkLabel?: string;
  // When true, renders without individual border/rounding for use inside a list sheet
  listItem?: boolean;
};

export function Card({
  cardClassName,
  contentClassName,
  href,
  children,
  dropdownItems,
  overlayed,
  linkLabel,
  listItem,
}: CardProps) {
  return (
    <div
      className={classNames(
        "group relative flex items-center gap-4 px-4",
        listItem
          ? "rounded-[inherit] bg-background-secondary hover:bg-background-tertiary"
          : "border rounded-xl bg-background-secondary hover:border-border-selected",
        cardClassName,
      )}
    >
      {href && (
        <Link
          href={href}
          className="absolute inset-0 z-0 rounded-[inherit] outline-none focus-visible:z-10 focus-visible:ring-2 focus-visible:ring-border-selected focus-visible:ring-inset"
        >
          <span className="sr-only">{linkLabel ?? "Open"}</span>
        </Link>
      )}
      <div
        className={classNames(
          "grow min-w-0 py-4",
          href && "cursor-pointer",
          contentClassName,
        )}
      >
        {children}
      </div>

      {overlayed}
      {dropdownItems && (
        <div className="relative z-10">
          <Menu
            placement="bottom-start"
            buttonProps={{
              "aria-label": "Open project settings",
              variant: "neutral",
              inline: true,
              icon: <DotsVerticalIcon className="text-content-secondary" />,
            }}
          >
            {dropdownItems?.map((item) => (
              <MenuItem
                key={item.text}
                variant={item.destructive ? "danger" : "default"}
                action={item.action}
                disabled={item.disabled}
                tip={item.tip}
                tipSide="right"
              >
                <item.Icon className="size-4" />
                {item.text}
              </MenuItem>
            ))}
          </Menu>
        </div>
      )}
    </div>
  );
}
