import React from "react";
import { CaretRightIcon } from "@radix-ui/react-icons";
import { Button } from "@ui/Button";
import { PalettePage, pageLabel } from "./pages";

const MAX_VISIBLE_CRUMBS = 3;

type Crumb = {
  label: string;
  // The drill-stack depth to return to when clicked.
  depth: number;
  tip?: string;
};

export function breadcrumbTrail(
  pages: PalettePage[],
  baseDepth: number,
): Crumb[] {
  const crumbs: Crumb[] = [];
  // The global palette is rooted at Home; a contextual menu is rooted at the
  // page it opened on, which is the last of its base pages.
  if (baseDepth === 0) {
    crumbs.push({ label: "Home", depth: 0 });
  } else {
    crumbs.push(pageCrumb(pages[baseDepth - 1], baseDepth));
  }
  const firstShown = Math.max(baseDepth, pages.length - MAX_VISIBLE_CRUMBS);
  const collapsed = pages.slice(baseDepth, firstShown);
  if (collapsed.length > 0) {
    crumbs.push({
      label: "…",
      depth: firstShown,
      tip: collapsed.map(pageLabel).join(" › "),
    });
  }
  for (let i = firstShown; i < pages.length; i++) {
    crumbs.push(pageCrumb(pages[i], i + 1));
  }
  return crumbs;
}

function pageCrumb(page: PalettePage, depth: number): Crumb {
  return { label: pageLabel(page), depth };
}

export function Breadcrumbs({
  pages,
  // The number of pages that form the fixed base of the current view. In the
  // global palette this is 0, so the trail is rooted at "Home". In a contextual
  // menu (anchored to a switcher) it's the depth the menu opened at, so the
  // trail is rooted at that menu's own base page instead of the whole palette.
  baseDepth = 0,
  onNavigate,
}: {
  pages: PalettePage[];
  baseDepth?: number;
  onNavigate: (depth: number) => void;
}) {
  const crumbs = breadcrumbTrail(pages, baseDepth);
  return (
    <div className="-mx-1 -mt-1.5 mb-1.5 flex animate-fadeInFromLoading items-center gap-1 border-b bg-background-tertiary/40 px-3 pt-3 pb-2 select-none">
      {crumbs.map((crumb, i) => (
        <React.Fragment key={`${crumb.depth}:${crumb.label}`}>
          {i > 0 && (
            <CaretRightIcon className="size-3 text-content-secondary" />
          )}
          {i === crumbs.length - 1 ? (
            <span className="max-w-48 truncate rounded-sm border bg-background-tertiary px-1.5 py-0.5 text-xs text-content-primary">
              {crumb.label}
            </span>
          ) : (
            <Crumb onClick={() => onNavigate(crumb.depth)} tip={crumb.tip}>
              {crumb.label}
            </Crumb>
          )}
        </React.Fragment>
      ))}
    </div>
  );
}

// A clickable crumb. Higher-contrast text than the current-page crumb plus a
// hover state so it reads as a "go back here" target.
function Crumb({
  children,
  onClick,
  tip,
}: {
  children: React.ReactNode;
  onClick: () => void;
  tip?: string;
}) {
  return (
    <Button
      variant="unstyled"
      onClick={onClick}
      tip={tip}
      className="max-w-48 truncate rounded-sm border bg-background-tertiary px-1.5 py-0.5 text-xs text-content-primary transition-colors hover:border-border-selected hover:bg-background-primary"
    >
      {children}
    </Button>
  );
}
