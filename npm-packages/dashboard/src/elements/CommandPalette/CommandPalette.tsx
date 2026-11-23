import { Command } from "cmdk";
import * as Dialog from "@radix-ui/react-dialog";
import { MagnifyingGlassIcon } from "@radix-ui/react-icons";
import { ErrorBoundary } from "@sentry/nextjs";
import React, { useCallback, useEffect, useRef, useState } from "react";
import {
  autoUpdate,
  offset,
  shift,
  size,
  useFloating,
  useMergeRefs,
} from "@floating-ui/react";
import { useRouter } from "next/router";
import { useHotkeys } from "react-hotkeys-hook";
import { createGlobalState, useClickAway } from "react-use";
import { cn } from "@ui/cn";
import { isApplePlatform } from "@ui/KeyboardShortcut";
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
import { createPaletteLinkRegistry, PaletteLinkContext } from "./newTab";
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

// The trigger to attach the palette to when it's opened from a switcher,
// rendering a compact menu next to the trigger instead of the centered dialog.
// `null` for the default centered dialog. Unlike the initial page, this
// persists for the whole open session so the trigger can show a selected state
// and the menu follows the trigger as the page scrolls or resizes.
export type PaletteAnchor = {
  reference: Element;
  // Which of the trigger's edges the menu lines up with: its left edge
  // ("start", the default) or its right edge ("end").
  align?: "start" | "end";
  // Which trigger opened it, so that trigger (and only it) can show a selected
  // state while several switchers share the header.
  source?: string;
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

function PaletteDialog(props: React.ComponentPropsWithRef<typeof Command>) {
  const label = "Convex Command Palette";

  // Keep the dialog and its title in the same Radix module instance:
  // cmdk's bundled dialog can use a separate instance with its own context.
  return (
    <Dialog.Root open>
      <Dialog.Portal>
        <Dialog.Overlay cmdk-overlay="" />
        <Dialog.Content cmdk-dialog="" aria-describedby={undefined}>
          <Dialog.Title className="sr-only">{label}</Dialog.Title>
          <Command {...props} label={label} />
        </Dialog.Content>
      </Dialog.Portal>
    </Dialog.Root>
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

  const { refs, floatingStyles, isPositioned, centered } =
    useAnchoredPosition(anchor);
  const ref = useRef<HTMLDivElement>(null);
  const rootRef = useMergeRefs([ref, refs.setFloating]);
  useClickAway(ref, onClose);

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
          typeof to === "string"
            ? to.split("#")[1]
            : to.hash?.replace(/^#/, "");
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
  const linkRegistry = useRef(createPaletteLinkRegistry()).current;

  const handleKeyDown = (event: React.KeyboardEvent) =>
    handlePaletteKeyDown(event, {
      inSubPage,
      search,
      popPage,
      onClose,
      armDrillModifier,
      confirmAction: confirmAction.current,
      copySelection: copyRegistry.copySelected,
      newTabOpenerForSelection: linkRegistry.newTabOpenerForSelection,
      isApple: isApplePlatform(),
    });

  return (
    <DrillModifierContext.Provider value={drillModifier}>
      <PaletteLoadingContext.Provider value={beginLoading}>
        <PaletteStatusContext.Provider value={setFooterStatus}>
          <PaletteConfirmContext.Provider value={setConfirmAction}>
            <PaletteCopyContext.Provider value={copyRegistry}>
              <PaletteLinkContext.Provider value={linkRegistry}>
                <PaletteDialog
                  ref={rootRef}
                  // No `loop`: with infinite-scroll lists, wrapping from the last
                  // loaded item back to the first snaps past not-yet-loaded pages,
                  // so arrow/Tab navigation stops at the ends instead.
                  filter={paletteFilter}
                  onKeyDown={handleKeyDown}
                  // When launched from a trigger, drop the centered layout and
                  // attach a compact menu to it, unless there's too little room
                  // around the trigger (see commandPalette.css).
                  className={cn(
                    // eslint-disable-next-line better-tailwindcss/no-unknown-classes -- custom class defined in commandPalette.css
                    anchor && "command-palette--anchored",
                    // eslint-disable-next-line better-tailwindcss/no-unknown-classes -- custom class defined in commandPalette.css
                    anchor && centered && "command-palette--centered",
                  )}
                  style={
                    anchor
                      ? {
                          ...(centered ? {} : floatingStyles),
                          width: `min(${ANCHORED_WIDTH}px, 100vw - ${2 * HORIZONTAL_EDGE_GAP}px)`,
                          ["--palette-min-height" as string]: `${MIN_MENU_HEIGHT}px`,
                          // Floating UI positions asynchronously, so hide the
                          // menu for the frame before its first position. With
                          // opacity rather than `visibility`, which would make
                          // the search input unfocusable when it autofocuses.
                          opacity: centered || isPositioned ? undefined : 0,
                        }
                      : undefined
                  }
                >
                  {inSubPage && (
                    <Breadcrumbs
                      pages={pages}
                      baseDepth={baseDepth}
                      onNavigate={goToDepth}
                    />
                  )}
                  <div
                    className={cn("relative -mx-1 -mt-1.5 flex items-center")}
                  >
                    {/* The input's top padding is larger than its bottom, so
                    center these on its text line (pt-4 + half of the 24px
                    line box, less half the icon) rather than on its box. */}
                    <MagnifyingGlassIcon className="pointer-events-none absolute top-4.5 left-3.5 size-5 text-content-tertiary" />
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
                      "-mx-1 scrollbar px-1 pt-1.5",
                      // An anchored menu's list ends with a wider gap so a list that
                      // shows all its items reads as finished, not cut off.
                      !hasPinnedActions && (anchor ? "pb-6" : "pb-2"),
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
                        {!isSearchPending && (
                          <AskAIQueryItem onClose={onClose} />
                        )}
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
                      <DeleteProjectsCommands
                        search={search}
                        onClose={onClose}
                      />
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
                </PaletteDialog>
              </PaletteLinkContext.Provider>
            </PaletteCopyContext.Provider>
          </PaletteConfirmContext.Provider>
        </PaletteStatusContext.Provider>
      </PaletteLoadingContext.Provider>
    </DrillModifierContext.Provider>
  );
}

const ANCHORED_WIDTH = 608; // 38rem
const ANCHOR_OFFSET = 8;
const HORIZONTAL_EDGE_GAP = 8;
// The far vertical edge gets a wide margin so a menu that reaches its maximum
// height visibly ends short of the viewport edge rather than looking cut off by
// it; the gap at the top edge only keeps a menu opened upward off the edge.
const TOP_EDGE_GAP = 16;
const BOTTOM_EDGE_GAP = 64;
// Below this much room under the trigger, the menu opens above it instead when
// there's more room there.
const MIN_COMFORTABLE_HEIGHT = 360;
// The shortest an anchored menu gets: room for its search input, footer and
// pinned actions plus a few rows. With less room than this on either side of
// the trigger, it opens centered in the viewport like the ⌘K palette instead,
// and it never shrinks below this (or the viewport, less its margins).
const MIN_MENU_HEIGHT = 320;

type AnchoredPlacement = "bottom" | "top" | "centered";

// Positions an anchored menu against its trigger. The placement is picked from
// the room around the trigger rather than by Floating UI's `flip`: the menu's
// height follows its content, so flipping on overflow would make it jump
// between sides as the user types. It's re-picked whenever the trigger may
// have moved: on window resize, and on Floating UI's updates (an ancestor
// scrolling, or the layout shifting) while the menu is attached.
//
// The first placement is picked during render, so the first position Floating
// UI computes already uses it; a placement corrected after mount would race
// that first computation, which can land last and leave the menu on the wrong
// side.
function useAnchoredPosition(anchor: PaletteAnchor | null) {
  // Tagged with the anchor it was measured for, so a new anchor is measured
  // during render instead of inheriting the previous one's placement.
  const [measured, setMeasured] = useState<{
    anchor: PaletteAnchor;
    placement: AnchoredPlacement;
  } | null>(null);
  const remeasure = useCallback(() => {
    if (!anchor) {
      return;
    }
    const placement = pickPlacement(anchor.reference);
    // Returning the previous state when nothing changed skips the re-render,
    // which matters since Floating UI calls this on every ancestor scroll.
    setMeasured((prev) =>
      prev?.anchor === anchor && prev.placement === placement
        ? prev
        : { anchor, placement },
    );
  }, [anchor]);
  useEffect(() => {
    if (!anchor) {
      return;
    }
    window.addEventListener("resize", remeasure);
    return () => window.removeEventListener("resize", remeasure);
  }, [anchor, remeasure]);
  const placement: AnchoredPlacement = !anchor
    ? "bottom"
    : measured?.anchor === anchor
      ? measured.placement
      : pickPlacement(anchor.reference);
  const centered = placement === "centered";
  const floating = useFloating({
    open: anchor !== null && !centered,
    elements: { reference: centered ? null : (anchor?.reference ?? null) },
    placement: `${centered ? "bottom" : placement}-${anchor?.align ?? "start"}`,
    strategy: "fixed",
    whileElementsMounted: (reference, floatingElement, update) =>
      autoUpdate(reference, floatingElement, () => {
        update();
        remeasure();
      }),
    middleware: [
      offset(ANCHOR_OFFSET),
      shift({ padding: HORIZONTAL_EDGE_GAP }),
      size({
        padding: { top: TOP_EDGE_GAP, bottom: BOTTOM_EDGE_GAP },
        apply({ availableHeight, elements }) {
          elements.floating.style.setProperty(
            "--palette-max-height",
            `${availableHeight}px`,
          );
        },
      }),
    ],
  });
  return { ...floating, centered };
}

function pickPlacement(reference: Element): AnchoredPlacement {
  const rect = reference.getBoundingClientRect();
  const below =
    window.innerHeight - rect.bottom - ANCHOR_OFFSET - BOTTOM_EDGE_GAP;
  const above = rect.top - ANCHOR_OFFSET - TOP_EDGE_GAP;
  if (Math.max(below, above) < MIN_MENU_HEIGHT) {
    return "centered";
  }
  return below >= MIN_COMFORTABLE_HEIGHT || below >= above ? "bottom" : "top";
}
