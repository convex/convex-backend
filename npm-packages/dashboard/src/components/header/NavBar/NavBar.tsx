import classNames from "classnames";
import Link from "next/link";
import { useEffect, useRef } from "react";

type NavItem = {
  label: string;
  href: string;
};

type NavBarProps = {
  items: NavItem[];
  activeLabel: string;
};

export function NavBar({ items, activeLabel }: NavBarProps) {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const activeLinkRef = useRef<HTMLAnchorElement | null>(null);
  const activeIndicatorRef = useRef<HTMLDivElement | null>(null);

  useEffect(() => {
    // Reposition indicator
    if (
      !activeLinkRef.current ||
      !activeIndicatorRef.current ||
      !parentRef.current
    ) {
      return;
    }

    const rect = activeLinkRef.current.getBoundingClientRect();
    const parentRect = parentRef.current.getBoundingClientRect();

    activeIndicatorRef.current.style.width = `100%`;
    activeIndicatorRef.current.style.transform = `translateX(${rect.x - parentRect.x}px) scaleX(${parentRect.width === 0 ? 0 : rect.width / parentRect.width})`;
  }, [activeLabel]);

  return (
    <div className="relative h-full">
      <div className="flex h-full truncate select-none" ref={parentRef}>
        {items.map(({ label, href }) => (
          <Link
            href={href}
            passHref
            ref={activeLabel === label ? activeLinkRef : undefined}
            className="group flex h-full items-center"
            key={label}
          >
            <div
              className={classNames(
                "p-2.5 mx-1 text-sm",
                "text-content-primary",
                "group-hover:bg-background-tertiary rounded-full",
                {
                  "font-medium": activeLabel === label,
                },
              )}
            >
              {label}
            </div>
          </Link>
        ))}

        <div
          className="absolute bottom-0 h-1 w-0 origin-top-left bg-content-secondary transition-transform will-change-transform motion-reduce:transition-none"
          ref={activeIndicatorRef}
        />
      </div>
    </div>
  );
}
