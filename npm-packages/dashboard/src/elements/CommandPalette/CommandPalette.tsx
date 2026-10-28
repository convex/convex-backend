import { Command } from "cmdk";
import { Title as DialogTitle } from "@radix-ui/react-dialog";
import { MagnifyingGlassIcon } from "@radix-ui/react-icons";
import { ErrorBoundary } from "@sentry/nextjs";
import React, { useCallback, useEffect, useRef, useState } from "react";
import { useRouter } from "next/router";
import { useHotkeys } from "react-hotkeys-hook";
import { createGlobalState, useClickAway, useWindowSize } from "react-use";
import { cn } from "@ui/cn";
import { useCurrentTeam } from "api/teams";
import { useCurrentProject } from "api/projects";
import { toast } from "@common/lib/utils";
import { isTextEditingTarget } from "@common/lib/isTextEditingTarget";
import { NavigationDestination, paletteFilter } from "./navigation";
import {
  DrillModifierContext,
  PaletteConfirmContext,
  PaletteLoadingContext,
  PaletteStatusContext,
} from "./items";
import { ComponentsCommands } from "./ComponentCommands";
import { DeleteProjectsCommands } from "./DeleteProjectsCommands";
import { ProjectCommands, SwitchDeploymentCommands } from "./ProjectCommands";
import { DeploymentCommands } from "./DeploymentCommands";
import { PickDeploymentCommands, PickProjectCommands } from "./PickerCommands";
import { DeploymentPicker, useCommandPaletteDeploymentPicker } from "./picker";
import { PalettePage, palettePlaceholder } from "./pages";
import { Breadcrumbs } from "./Breadcrumbs";
import { Footer } from "./Footer";
import { NoResultsMessage } from "./NoResultsMessage";
import { AskAIQueryItem, RootCommands } from "./RootCommands";
import {
  DocumentSearchResult,
  DocumentSearchResultItem,
} from "./DocumentSearchResult";
import { useDrillStack } from "./useDrillStack";
import { SwitchProjectCommands, TeamDeploymentsCommands } from "./searchGroups";
import { ThemeCommands } from "./ThemeCommands";
import { TeamsCommands } from "./TeamsCommands";
import { handlePaletteKeyDown } from "./keyboard";
import { createPaletteCopyRegistry, PaletteCopyContext } from "./copy";
import { usePaletteAnalytics } from "./analytics";

export const useCommandPaletteOpen = createGlobalState(false);

// A one-shot drill-in stack for the palette's next open (e.g. the top-left
// project switcher opens it directly on the "Switch Project" view). More than
// one page opens the menu partway down its own stack, so the user can step back
// up it — the deployment picker opens on a project's deployments with the
// project list behind it. The dialog consumes this on mount and clears it, so a
// subsequent open via ⌘K/slash starts at the root as usual.
export const useCommandPaletteInitialPages = createGlobalState<
  PalettePage[] | null
>(null);

// The viewport point (a trigger's bottom-left) to anchor the palette under when
// it's opened from the project switcher, rendering a compact menu attached to
// the trigger instead of the centered dialog. `null` for the default centered
// dialog. Unlike the initial page, this persists for the whole open session so
// the trigger can show a selected state and the dialog stays anchored.
export type PaletteAnchor = {
  left: number;
  top: number;
  // Which trigger opened it, so that trigger (and only it) can show a selected
  // state while several switchers share the header.
  source?: string;
  // Pin the menu to this width (px) to match the trigger it hangs off of;
  // defaults to the anchored width in commandPalette.css when unset.
  width?: number;
};
export const useCommandPaletteAnchor = createGlobalState<PaletteAnchor | null>(
  null,
);

// Opens the command palette, optionally drilled into a stack of nested pages,
// anchored beneath a trigger, and/or in picker mode (where the caller, rather
// than the palette, decides what selecting a deployment does).
export function useOpenCommandPalette() {
  const [, setOpen] = useCommandPaletteOpen();
  const [, setInitialPages] = useCommandPaletteInitialPages();
  const [, setAnchor] = useCommandPaletteAnchor();
  const [, setPicker] = useCommandPaletteDeploymentPicker();
  return useCallback(
    (options?: {
      pages?: PalettePage[];
      anchor?: PaletteAnchor;
      picker?: DeploymentPicker;
    }) => {
      setInitialPages(options?.pages ?? null);
      setAnchor(options?.anchor ?? null);
      setPicker(options?.picker ?? null);
      setOpen(true);
    },
    [setOpen, setInitialPages, setAnchor, setPicker],
  );
}

export function CommandPalette() {
  const [open, setOpen] = useCommandPaletteOpen();
  const [, setAnchor] = useCommandPaletteAnchor();
  const [, setPicker] = useCommandPaletteDeploymentPicker();
  const router = useRouter();

  const [detail, setDetail] = useState<DocumentSearchResultItem | null>(null);
  const { trackOpened } = usePaletteAnalytics();

  // Closing always clears any trigger anchor and picker so the next
  // keyboard-driven open is the centered, navigating dialog rather than
  // re-anchoring to the switcher or menu that opened it last.
  const closePalette = useCallback(() => {
    setOpen(false);
    setAnchor(null);
    setPicker(null);
  }, [setOpen, setAnchor, setPicker]);

  useHotkeys(
    ["meta+k", "ctrl+k"],
    (event) => {
      event.preventDefault();
      if (!open) {
        trackOpened("hotkey");
      }
      setAnchor(null);
      setPicker(null);
      setOpen((isOpen) => !isOpen);
    },
    // Allows this shortcut to work even if you're focusing a form element
    { enableOnFormTags: true },
  );

  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key !== "/" || event.metaKey || event.ctrlKey || event.altKey) {
        return;
      }
      // Don't steal "/" while the user is typing in a field.
      if (isTextEditingTarget(document.activeElement)) {
        return;
      }
      event.preventDefault();
      if (!open) {
        trackOpened("slash");
      }
      setAnchor(null);
      setPicker(null);
      setOpen(true);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, setOpen, setAnchor, setPicker, trackOpened]);

  return (
    <>
      {open && (
        <ErrorBoundary
          onError={() => {
            closePalette();
            toast(
              "error",
              "Something went wrong with the command palette. Please try again.",
            );
          }}
        >
          <CommandPaletteDialog
            onClose={closePalette}
            onOpenDetail={(item) => {
              setDetail(item);
              closePalette();
            }}
          />
        </ErrorBoundary>
      )}
      {detail && (
        <DocumentSearchResult
          detail={detail}
          onClose={() => setDetail(null)}
          onNavigate={(to) => {
            setDetail(null);
            void router.push(to);
          }}
        />
      )}
    </>
  );
}

function CommandPaletteDialog({
  onClose,
  onOpenDetail,
}: {
  onClose: () => void;
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
}) {
  const router = useRouter();
  const team = useCurrentTeam();
  const project = useCurrentProject();
  // Switching submenus (drilling in/out, or jumping via breadcrumbs) can move
  // focus onto the clicked row or breadcrumb. The search input stays mounted
  // across page changes, so returning focus to it keeps the user typing.
  const inputRef = useRef<HTMLInputElement>(null);
  // cmdk only ever scrolls the selected row into view, so a list left scrolled
  // down stays there through whatever replaces its contents.
  const listRef = useRef<HTMLDivElement>(null);
  const scrollListToTop = useCallback(() => {
    if (listRef.current) {
      listRef.current.scrollTop = 0;
    }
  }, []);
  const afterNavigate = useCallback(() => {
    scrollListToTop();
    inputRef.current?.focus();
  }, [scrollListToTop]);

  // "Drilling" is stepping into a nested view of the palette rather than
  // navigating away (e.g. from the root into a team's list of projects, or
  // from a project into its deployments).
  const [initialPages, setInitialPages] = useCommandPaletteInitialPages();
  const [anchor] = useCommandPaletteAnchor();
  const [picker] = useCommandPaletteDeploymentPicker();
  const { pages, search, setSearch, pushPage, popPage, goToDepth, resetTo } =
    useDrillStack({ initialPages: initialPages ?? [], afterNavigate });
  useEffect(() => {
    if (initialPages) {
      resetTo(initialPages);
      setInitialPages(null);
    }
  }, [initialPages, setInitialPages, resetTo]);
  // `drillPage` is the view currently shown
  const drillPage = pages[pages.length - 1];
  const placeholder = palettePlaceholder(drillPage, team?.name, project?.name);

  // The Switch Team / Switch Project pages pin a create-action bar to the
  // bottom of the list (see PinnedActions). The bar sticks flush to the bottom,
  // so drop the list's own bottom padding on those pages to avoid a gap beneath
  // it; keep the padding elsewhere for breathing room above the footer. The bar
  // also overlays a row scrolled to the bottom — cmdk scrolls the active row
  // with scrollIntoView({ block: "nearest" }), which honors scroll-padding, so
  // reserve scroll-padding-bottom for the bar so the selected row stays clear
  // of it. The bar is a fixed single row, so this is a static height rather
  // than a measured one.
  const hasPinnedActions =
    drillPage?.type === "teams" || drillPage?.type === "projects";

  // Keep an anchored menu fully on-screen: cap its width to the viewport and
  // clamp its left edge so it never spills past either side — a right-aligned
  // trigger or a narrow screen would otherwise push it off-frame. Falls back to
  // the 38rem / 0.5rem-inset defaults from commandPalette.css.
  const { width: viewportWidth } = useWindowSize();
  const anchorStyle = (() => {
    if (!anchor) {
      return undefined;
    }
    const EDGE_GAP = 8;
    const DEFAULT_WIDTH = 608; // 38rem
    const width = Math.min(
      anchor.width ?? DEFAULT_WIDTH,
      viewportWidth - 2 * EDGE_GAP,
    );
    const left = Math.max(
      EDGE_GAP,
      Math.min(anchor.left, viewportWidth - width - EDGE_GAP),
    );
    return { left, top: anchor.top, width };
  })();

  // Anchored mode opens directly on a drill page (its base), so "going back"
  // and the breadcrumbs only apply once the user has drilled *past* that base.
  // At the base, Escape closes and no breadcrumbs show.
  const baseDepth = anchor ? 1 : 0;
  const inSubPage = pages.length > baseDepth;

  // "Contextual" = the page is the base of a menu anchored to a header switcher
  // (vs. drilled into from the main ⌘K palette). The Switch Team / Switch
  // Deployment pages only show their settings shortcut (Team / Project
  // Settings) in that contextual case, where it doubles as the switcher's menu.
  const contextual = anchor !== null && !inSubPage;

  // A status line the active page can publish into the footer's right gutter.
  const [footerStatus, setFooterStatus] = useState<React.ReactNode>(null);

  const confirmAction = useRef<(() => void) | null>(null);
  const setConfirmAction = useCallback((action: (() => void) | null) => {
    confirmAction.current = action;
  }, []);

  const [loadingCount, setLoadingCount] = useState(0);
  const beginLoading = useCallback(() => {
    setLoadingCount((count) => count + 1);
    return () => setLoadingCount((count) => count - 1);
  }, []);
  const isSearchPending = loadingCount > 0;

  const onNavigate = useCallback(
    (to: NavigationDestination) => {
      onClose();
      void router.push(to).then(() => {
        // For section targets, scroll the section into view once the
        // destination has rendered. This also covers re-selecting the section
        // you're already on, which is a no-op for the router.
        const hash =
          typeof to === "string" && to.includes("#")
            ? to.split("#")[1]
            : undefined;
        if (hash) {
          setTimeout(() => {
            document
              .getElementById(hash)
              ?.scrollIntoView({ behavior: "smooth", block: "start" });
          }, 100);
        }
      });
    },
    [router, onClose],
  );

  const ref = useRef<HTMLDivElement>(null);
  useClickAway(ref, onClose);

  // Used as a signal to what action should be performed by the selected list item in the palette.
  // Updated when the event handler detects the user is using a modifier key.
  const drillModifier = useRef(false);
  const armDrillModifier = (active: boolean) => {
    drillModifier.current = active;
    setTimeout(() => {
      drillModifier.current = false;
    }, 0);
  };

  const copyRegistry = useRef(createPaletteCopyRegistry()).current;

  const handleKeyDown = (event: React.KeyboardEvent) =>
    handlePaletteKeyDown(event, {
      inSubPage,
      search,
      popPage,
      onClose,
      armDrillModifier,
      confirmAction: confirmAction.current,
      copySelection: copyRegistry.copySelected,
    });

  return (
    <DrillModifierContext.Provider value={drillModifier}>
      <PaletteLoadingContext.Provider value={beginLoading}>
        <PaletteStatusContext.Provider value={setFooterStatus}>
          <PaletteConfirmContext.Provider value={setConfirmAction}>
            <PaletteCopyContext.Provider value={copyRegistry}>
              <Command.Dialog
                open
                ref={ref}
                label="Convex Command Palette"
                // No `loop`: with infinite-scroll lists, wrapping from the last
                // loaded item back to the first snaps past not-yet-loaded pages,
                // so arrow/Tab navigation stops at the ends instead.
                filter={paletteFilter}
                onKeyDown={handleKeyDown}
                // When launched from a trigger, drop the centered layout and
                // attach a compact menu just below it (see commandPalette.css).
                // eslint-disable-next-line better-tailwindcss/no-unknown-classes -- custom class defined in commandPalette.css
                className={anchor ? "command-palette--anchored" : undefined}
                style={anchorStyle}
              >
                {/* cmdk renders a Radix Dialog with only an aria-label; Radix still
            requires a Dialog.Title inside the content for screen readers, so
            provide a visually hidden one. */}
                <DialogTitle className="sr-only">
                  Convex Command Palette
                </DialogTitle>
                {inSubPage && (
                  <Breadcrumbs
                    pages={pages}
                    baseDepth={baseDepth}
                    onNavigate={goToDepth}
                  />
                )}
                <div
                  className={cn(
                    "relative -mx-1 -mt-1.5 mb-1.5 flex items-center",
                  )}
                >
                  {/* The input's top padding is larger than its bottom, so
                    center these on its text line (pt-4 + half of the 20px
                    line box, less half the icon) rather than on its box. */}
                  <MagnifyingGlassIcon className="pointer-events-none absolute top-4.5 left-3 size-4 text-content-tertiary" />
                  <Command.Input
                    ref={inputRef}
                    autoFocus
                    placeholder={placeholder}
                    value={search}
                    onValueChange={(value) => {
                      setSearch(value);
                      scrollListToTop();
                    }}
                  />
                  {isSearchPending && (
                    <div
                      role="progressbar"
                      aria-label="Loading results"
                      className="pointer-events-none absolute inset-x-0 bottom-0 h-0.5 animate-fadeInFromLoading overflow-hidden"
                    >
                      <div className="h-full w-1/5 animate-indeterminateBar rounded-full bg-util-accent/70" />
                    </div>
                  )}
                </div>
                {/* While searching, cmdk re-sorts and reparents every group/item on
                each keystroke, which restarts their load-in fade animation. This
                attribute drives the CSS rule that suppresses that fade so results
                don't flash on every character. */}
                {/* Bleed the list to the palette's edges (its content is padded
                  back by px-1) so a pinned bar inside it can span the full
                  width; without this the list's overflow clips the bleed. */}
                <Command.List
                  ref={listRef}
                  className={cn(
                    "-mx-1 scrollbar px-1",
                    !hasPinnedActions && "pb-2",
                    // Flex-fills the sizer so the pinned create bar sits at the
                    // list bottom (see commandPalette.css).
                    // eslint-disable-next-line better-tailwindcss/no-unknown-classes -- custom class defined in commandPalette.css
                    hasPinnedActions && "command-palette-list--pinned",
                  )}
                  // Clear the sticky create bar (~44px + a little gap); overrides
                  // the stylesheet's scroll-pb-2 only while the bar is up.
                  style={
                    hasPinnedActions
                      ? { scrollPaddingBottom: "3.5rem" }
                      : undefined
                  }
                  data-searching={search ? "" : undefined}
                >
                  {!isSearchPending && (
                    <Command.Empty>
                      <NoResultsMessage onClose={onClose} />
                    </Command.Empty>
                  )}
                  {drillPage === undefined && (
                    <>
                      <RootCommands
                        search={search}
                        onNavigate={onNavigate}
                        onOpenDetail={onOpenDetail}
                        pushPage={pushPage}
                        onClose={onClose}
                      />
                      {!isSearchPending && <AskAIQueryItem onClose={onClose} />}
                    </>
                  )}
                  {drillPage?.type === "teams" && (
                    <TeamsCommands
                      onNavigate={onNavigate}
                      onClose={onClose}
                      contextual={contextual}
                    />
                  )}
                  {drillPage?.type === "projects" && (
                    <SwitchProjectCommands
                      search={search}
                      onNavigate={onNavigate}
                      pushPage={pushPage}
                      onClose={onClose}
                    />
                  )}
                  {drillPage?.type === "components" && (
                    <ComponentsCommands onClose={onClose} />
                  )}
                  {drillPage?.type === "theme" && (
                    <ThemeCommands onClose={onClose} />
                  )}
                  {drillPage?.type === "deleteProjects" && (
                    <DeleteProjectsCommands search={search} onClose={onClose} />
                  )}
                  {drillPage?.type === "project" && (
                    <ProjectCommands
                      project={drillPage.project}
                      onNavigate={onNavigate}
                      onSelectDeployment={(deployment) =>
                        pushPage({
                          type: "deployment",
                          deployment,
                          projectSlug: drillPage.project.slug,
                        })
                      }
                    />
                  )}
                  {drillPage?.type === "deployments" && (
                    <SwitchDeploymentCommands
                      project={drillPage.project}
                      onNavigate={onNavigate}
                      contextual={contextual}
                      onSelectDeployment={(deployment) =>
                        pushPage({
                          type: "deployment",
                          deployment,
                          projectSlug: drillPage.project.slug,
                        })
                      }
                    />
                  )}
                  {drillPage?.type === "teamDeployments" && (
                    <TeamDeploymentsCommands
                      search={search}
                      onNavigate={onNavigate}
                      pushPage={pushPage}
                    />
                  )}
                  {drillPage?.type === "deployment" && (
                    <DeploymentCommands
                      deployment={drillPage.deployment}
                      projectSlug={drillPage.projectSlug}
                      onNavigate={onNavigate}
                    />
                  )}
                  {drillPage?.type === "pickDeployment" && picker && (
                    <PickDeploymentCommands
                      project={drillPage.project}
                      picker={picker}
                      onSelect={(deployment) => {
                        onClose();
                        picker.onSelect(deployment);
                      }}
                    />
                  )}
                  {drillPage?.type === "pickProject" && (
                    <PickProjectCommands
                      search={search}
                      pinnedProject={picker?.selectedProject}
                      onSelectProject={(project) =>
                        pushPage({ type: "pickDeployment", project })
                      }
                    />
                  )}
                </Command.List>
                <Footer inSubPage={inSubPage} status={footerStatus} />
              </Command.Dialog>
            </PaletteCopyContext.Provider>
          </PaletteConfirmContext.Provider>
        </PaletteStatusContext.Provider>
      </PaletteLoadingContext.Provider>
    </DrillModifierContext.Provider>
  );
}
