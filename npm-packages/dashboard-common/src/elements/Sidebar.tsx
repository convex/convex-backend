import classNames from "classnames";
import { useRouter } from "next/router";
import { ReactNode } from "react";
import { useWindowSize } from "react-use";
import omit from "lodash/omit";
import {
  DoubleArrowLeftIcon,
  DoubleArrowRightIcon,
} from "@radix-ui/react-icons";
import { TooltipSide } from "@ui/Tooltip";
import { Button } from "@ui/Button";

export type SidebarItem = {
  key: string | null;
  label: string;
  Icon: React.FC<{ className?: string }>;
  href: string;
  isActive?: (currentPage: string) => boolean;
  disabled?: boolean;
  /** muted = disabled appearance but still a clickable link */
  muted?: boolean;
  tooltip?: string;
  target?: "_blank";
};

export type SidebarGroup = {
  key: string;
  items: SidebarItem[];
};

export function Sidebar({
  items,
  collapsed,
  setCollapsed,
  header,
}: {
  collapsed: boolean;
  setCollapsed: (collapsed: boolean) => void;
  items: SidebarGroup[];
  header?: ReactNode;
}) {
  const currentPage = useCurrentPage();
  const { width } = useWindowSize();

  return (
    <aside
      className={classNames(
        "bg-background-secondary animate-fadeInFromLoading",
        "shadow-sm border-b sm:border-b-0 sm:border-r transition-[min-width]",
        "px-3 py-2 w-0 overflow-auto scrollbar-none",
        "z-40 w-full min-h-fit sm:w-fit sm:h-full",
        "flex flex-row sm:flex-col justify-between",
        "shrink-0",
        { [`min-w-[20px]`]: collapsed },
        { [`min-w-[130px]`]: !collapsed },
      )}
    >
      <div className="flex gap-1 sm:flex-col">
        {header}

        <div className="flex sm:flex-col sm:divide-x-0 sm:divide-y">
          {items.map((group) => (
            <div key={group.key} className="flex gap-1 sm:flex-col sm:py-2">
              {group.items.map((item) => (
                <div className="relative h-7.5" key={item.key}>
                  <SidebarLink
                    {...omit(item, "key")}
                    collapsed={collapsed}
                    isActive={currentPage === item.key}
                    disabled={item.disabled}
                    small
                    tip={
                      item.tooltip
                        ? item.tooltip
                        : collapsed
                          ? item.label
                          : undefined
                    }
                    tipSide={width > 640 ? "right" : "bottom"}
                  >
                    {item.label}
                  </SidebarLink>
                </div>
              ))}
            </div>
          ))}
        </div>
      </div>

      <Button
        variant="unstyled"
        onClick={() => setCollapsed(!collapsed)}
        aria-label={collapsed ? "Expand" : "Collapse"}
        tip={collapsed ? "Expand" : undefined}
        tipSide="right"
        className={classNames(
          sidebarLinkClassNames({
            small: true,
          }),
          "sm:flex hidden",
        )}
        icon={collapsed ? <DoubleArrowRightIcon /> : <DoubleArrowLeftIcon />}
      >
        {!collapsed && "Collapse"}
      </Button>
    </aside>
  );
}

export function SidebarLink({
  collapsed,
  href,
  query,
  children,
  Icon,
  isActive,
  disabled,
  muted,
  proBadge,
  small,
  tip,
  tipSide,
  target,
}: {
  collapsed?: boolean;
  href: string;
  query?: Record<string, string>;
  children: ReactNode;
  isActive: boolean;
  Icon?: React.FC<{ className?: string }>;
  disabled?: boolean;
  muted?: boolean;
  proBadge?: boolean;
  small?: boolean;
  tip?: string;
  tipSide?: TooltipSide;
  target?: "_blank";
}) {
  const { query: currentQuery } = useRouter();
  return (
    <Button
      tip={tip}
      tipSide={tipSide}
      variant="unstyled"
      href={
        disabled
          ? undefined
          : {
              pathname: href,
              query: currentQuery.component
                ? { ...query, component: currentQuery.component }
                : query,
            }
      }
      aria-disabled={disabled}
      className={sidebarLinkClassNames({
        isActive,
        isMuted: muted || disabled,
        isDisabled: disabled,
        small,
      })}
      target={target}
    >
      {Icon && (
        <Icon
          className={classNames(
            "size-[1.125rem] shrink-0 min-h-[1.125rem]",
            !collapsed && "text-content-secondary",
          )}
          aria-hidden
        />
      )}
      <span className={classNames("select-none", collapsed && "sm:hidden")}>
        {children}
      </span>
      {proBadge && (
        <span
          className="rounded-sm bg-util-accent px-1.5 py-0.5 text-xs font-semibold tracking-wider text-white uppercase"
          title="Only available on the Pro plan"
        >
          Pro
        </span>
      )}
    </Button>
  );
}

export function sidebarLinkClassNames(props: {
  // defaults to false
  isActive?: boolean;
  // default to false
  isMuted?: boolean;
  // default to false
  isDisabled?: boolean;
  // defaults to normal sans font
  font?: "mono";
  small?: boolean;
  // defaults to true
  fitWidth?: boolean;
}) {
  let fontSize = props.small ? "text-[13px]" : "text-sm";
  if (props.font === "mono") {
    fontSize = props.small ? "text-xs" : "text-[0.8125rem]";
  }
  return classNames(
    "w-full",
    "rounded-sm flex items-center gap-2 whitespace-nowrap",
    "text-content-primary",
    fontSize,
    (props.fitWidth ?? true) ? "min-w-fit" : null,
    props.font === "mono" && "font-mono px-1 py-1",
    props.small ? "p-1.5" : "px-3 py-2",
    props.isDisabled
      ? "cursor-not-allowed"
      : "cursor-pointer hover:bg-background-primary",
    "focus-visible:outline-0 focus-visible:ring-2 focus-visible:ring-util-accent",
    (props.isActive ?? false) ? "font-semibold bg-background-tertiary" : null,
    props.isMuted && !props.isActive ? "text-content-tertiary" : null,
  );
}

export function useCurrentPage() {
  const router = useRouter();

  const path = router.pathname
    .replace("/t/[team]/[project]/[deploymentName]", "")
    .split("/")
    .filter((i) => !!i);
  return path[0] ?? "health";
}
