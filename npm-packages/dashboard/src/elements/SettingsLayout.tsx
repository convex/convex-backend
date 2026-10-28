import { cn } from "@ui/cn";
import { useEffect, useState, type ComponentType, type ReactNode } from "react";

export type SettingsSection = {
  id: string;
  label: string;
  Icon: ComponentType<{
    className?: string;
    width?: string | number;
    height?: string | number;
  }>;
};

// Two-column settings layout with a sticky, scroll-tracking sidebar navigation.
// Section anchor ids are shared with the command palette (which deep-links to
// them) via `lib/sectionAnchors`, so pass the same ids used to tag `children`.
export function SettingsLayout({
  title,
  sections,
  // Toggling this from false to true (once the page's data has loaded and the
  // section elements exist) triggers the initial scroll to `location.hash`.
  contentReady,
  children,
}: {
  title: ReactNode;
  sections: SettingsSection[];
  contentReady: boolean;
  children: ReactNode;
}) {
  useEffect(() => {
    if (!contentReady) return;
    if (typeof window !== "undefined" && window.location.hash) {
      const id = window.location.hash.slice(1);
      const element = document.getElementById(id);
      if (element) {
        // Wait for the section to render before scrolling to it.
        setTimeout(() => {
          element.scrollIntoView({
            behavior: "smooth",
            block: "start",
            inline: "start",
          });
        }, 100);
      }
    }
  }, [contentReady]);

  const titleNode = <h2 className="pointer-events-auto py-6">{title}</h2>;

  return (
    <div className="relative h-full [--container-px:--spacing(6)] [--container-width:80rem] [--sidebar-gap:--spacing(8)] [--sidebar-width:14rem]">
      <div className="pointer-events-none absolute inset-0 top-0 z-10 hidden md:block">
        <div className="mx-auto flex h-full max-w-(--container-width) gap-(--sidebar-gap) px-(--container-px)">
          <div className="h-full w-(--sidebar-width)">
            <div className="grid h-full grid-rows-[auto_1fr]">
              {titleNode}
              <div className="scrollbar overflow-y-auto">
                <div className="pointer-events-auto pb-8">
                  <SettingsNavigation sections={sections} />
                </div>
              </div>
            </div>
          </div>
          <div className="grow" />
        </div>
      </div>
      <div
        className="scrollbar h-full overflow-y-auto"
        data-settings-content-wrapper
      >
        <div className="m-auto flex min-h-0 max-w-(--container-width) gap-(--sidebar-gap) px-(--container-px)">
          <div className="hidden w-(--sidebar-width) shrink-0 md:block" />

          <div className="flex grow flex-col items-start">
            <div className="md:hidden">{titleNode}</div>

            <div
              data-settings-content
              className="flex w-full grow flex-col gap-6 pr-2 pb-6 *:scroll-mt-3 md:pt-20"
            >
              {children}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}

function SettingsNavigation({ sections }: { sections: SettingsSection[] }) {
  return (
    <nav
      data-settings-nav
      className="relative"
      aria-label="Settings navigation"
    >
      <div
        className="absolute left-0 h-full w-0.5 rounded-sm bg-background-tertiary"
        aria-hidden="true"
      />
      <SettingsNavigationScrollProgress sections={sections} />
      <ul className="pl-1 text-sm">
        {sections.map(({ id, label, Icon }) => (
          <li key={id} className="py-px">
            <a
              href={`#${id}`}
              className={cn(
                "flex items-center gap-2 rounded-sm p-2 transition-all duration-200",
                "text-content-primary hover:bg-background-secondary",
              )}
              onClick={(e) => {
                e.preventDefault();
                const element = document.getElementById(id);
                if (element) {
                  const rect = element.getBoundingClientRect();
                  const isInView =
                    rect.top >= 0 && rect.bottom <= window.innerHeight;
                  element.scrollIntoView({
                    behavior: "smooth",
                    block: isInView ? "start" : "nearest",
                    inline: "nearest",
                  });

                  window.history.pushState(null, "", `#${id}`);
                }
              }}
            >
              <Icon
                width={18}
                height={18}
                className="size-4.5 max-h-4.5 min-h-4.5 max-w-4.5 shrink-0 text-content-secondary"
                aria-hidden
              />
              {label}
            </a>
          </li>
        ))}
      </ul>
    </nav>
  );
}

function SettingsNavigationScrollProgress({
  sections,
}: {
  sections: SettingsSection[];
}) {
  const [transform, setTransform] = useState<string | undefined>(undefined);

  useEffect(() => {
    const contentWrapper = document.querySelector(
      "[data-settings-content-wrapper]",
    );
    const content = document.querySelector("[data-settings-content]");
    if (!contentWrapper) return undefined;

    const forceUpdate = () => {
      // Don't show indicator until sections are rendered
      const firstElement = document.getElementById(sections[0].id);
      if (!firstElement) {
        setTransform(undefined);
        return;
      }

      const containerRect = contentWrapper.getBoundingClientRect();

      const elementHeight = 1 / sections.length;

      const firstBoundary = findScrollBoundary(
        "first",
        containerRect,
        sections,
      );
      const lastBoundary = findScrollBoundary("last", containerRect, sections);

      const y =
        (firstBoundary.index + firstBoundary.topClippedFraction) *
        elementHeight;
      const height =
        firstBoundary.index === lastBoundary.index
          ? firstBoundary.visibilityFraction * elementHeight
          : (firstBoundary.visibilityFraction +
              lastBoundary.visibilityFraction +
              lastBoundary.index -
              firstBoundary.index -
              1) *
            elementHeight;

      setTransform(`translateY(${y * 100}%) scaleY(${height})`);
    };

    forceUpdate(); // Initial calculation

    const update = () => {
      window.requestAnimationFrame(forceUpdate);
    };
    contentWrapper.addEventListener("scroll", update);
    window.addEventListener("resize", update);

    const resizeObserver = new ResizeObserver(update);
    if (content) {
      resizeObserver.observe(content);
    }

    return () => {
      contentWrapper.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      resizeObserver.disconnect();
    };
  }, [sections]);

  if (transform === undefined) return null;

  return (
    <div
      className="absolute left-0 h-full w-0.5 origin-top rounded-sm bg-content-primary"
      style={{
        transform,
      }}
      aria-hidden="true"
    />
  );
}

function findScrollBoundary(
  boundary: "first" | "last",
  containerRect: DOMRect,
  sections: SettingsSection[],
) {
  for (
    let i = boundary === "first" ? 0 : sections.length - 1;
    boundary === "first" ? i < sections.length : i >= 0;
    boundary === "first" ? i++ : i--
  ) {
    const section = sections[i];
    const element = document.getElementById(section.id);
    if (!element) {
      continue;
    }

    const rect = element.getBoundingClientRect();

    const visibleHeight =
      Math.min(rect.bottom, containerRect.bottom) -
      Math.max(rect.top, containerRect.top);

    if (visibleHeight > 0) {
      const elementHeight = rect.height;
      return {
        index: i,
        visibilityFraction: visibleHeight / elementHeight,
        topClippedFraction:
          Math.max(0, containerRect.top - rect.top) / elementHeight,
      };
    }
  }

  return {
    index: 0,
    visibilityFraction: 0,
    topClippedFraction: 0,
  };
}
