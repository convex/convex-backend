import { Command, useCommandState } from "cmdk";
import React, { useContext } from "react";
import type { UrlObject } from "url";
import { ConvexProvider, useQuery } from "convex/react";
import { CaretSortIcon } from "@radix-ui/react-icons";
import { useRouter } from "next/router";
import udfs from "@common/udfs";
import {
  DeploymentInfoContext,
  useMaybeConnectedDeployment,
} from "@common/lib/deploymentContext";
import { PuzzlePieceIcon } from "@common/elements/icons";
import type { Nent } from "@common/lib/useNents";
import {
  ActionItem,
  CurrentBadge,
  HighlightedText,
  ItemPrimary,
  LoadingSignal,
} from "./items";
import { useCopyAction } from "./copy";
import {
  componentQuery,
  matchesSearch,
  REMOTE_VALUE_PREFIX,
} from "./navigation";
import { sortByMatch } from "./matchRank";
import { usePaletteAnalytics } from "./analytics";

// The deployment pages that render a NentSwitcher, i.e. where the `component`
// query param has an effect. Suffixes of the Next.js route pattern after
// `[deploymentName]`.
const NENT_AWARE_PAGE_SUFFIXES = new Set([
  "/data",
  "/functions",
  "/files",
  "/schema",
  "/schedules/functions",
  "/schedules/crons",
]);

export function pageHasNentSwitcher(pathname: string): boolean {
  const [, suffix] = pathname.split("/[deploymentName]");
  return suffix !== undefined && NENT_AWARE_PAGE_SUFFIXES.has(suffix);
}

// Where viewing a component leads. On a nent-aware page (Data, Functions, …)
// that's the same page with the `component` query param set. Elsewhere that
// param has no effect, so it's the component's Data page instead.
export function useComponentHref() {
  const router = useRouter();
  const { deploymentsURI } = useContext(DeploymentInfoContext);
  return (id: string | null): UrlObject => {
    if (!pageHasNentSwitcher(router.pathname)) {
      return { pathname: `${deploymentsURI}/data`, query: componentQuery(id) };
    }
    const { component: _, ...query } = router.query;
    // `router.pathname` is the route pattern; next/link and router.push both
    // interpolate its dynamic segments from `query`.
    return {
      pathname: router.pathname,
      query: { ...query, ...componentQuery(id) },
    };
  };
}

export function useSelectComponent(onClose: () => void) {
  const router = useRouter();
  const href = useComponentHref();
  return (id: string | null) => {
    onClose();
    // Staying on the same page only moves a query param, so keep the current
    // page's data instead of re-running its server-side fetch. True exactly
    // when `useComponentHref` took its same-pathname branch.
    void router.push(href(id), undefined, {
      shallow: pageHasNentSwitcher(router.pathname),
    });
  };
}

export function useComponents(): Nent[] | undefined {
  const { useIsOperationAllowed } = useContext(DeploymentInfoContext);
  const canViewData = useIsOperationAllowed("ViewData");
  const components = useQuery(udfs.components.list, canViewData ? {} : "skip");
  // The unnamed root definition is the app itself, not a component.
  return components?.filter((component) => component.name !== null) as
    | Nent[]
    | undefined;
}

// The root-list "Components" group: the "Switch Component…" drill plus, while
// searching, a "Switch to <component>" shortcut per component. Rendered on any
// deployment page — useSelectComponent redirects to the Data page when the
// current page can't switch components in place.
export function ComponentSwitchCommands({
  search,
  onOpenComponentsPage,
  onClose,
}: {
  search: string;
  onOpenComponentsPage: () => void;
  onClose: () => void;
}) {
  const connected = useMaybeConnectedDeployment();
  if (!connected?.deployment) {
    return null;
  }
  return (
    <ConvexProvider client={connected.deployment.client}>
      <ComponentSwitchInner
        search={search}
        onOpenComponentsPage={onOpenComponentsPage}
        onClose={onClose}
      />
    </ConvexProvider>
  );
}

function ComponentSwitchInner({
  search,
  onOpenComponentsPage,
  onClose,
}: {
  search: string;
  onOpenComponentsPage: () => void;
  onClose: () => void;
}) {
  const router = useRouter();
  const components = useComponents();
  const selected =
    typeof router.query.component === "string" ? router.query.component : null;
  const selectComponent = useSelectComponent(onClose);
  if (!components || components.length === 0) {
    return null;
  }
  const trimmed = search.trim();
  // The root app plus every named component, as switch targets (id null = app).
  const targets: {
    id: string | null;
    path: string;
    // What results are ranked on: the target's own names, not the generic
    // "component" keyword that every component shares.
    names: string[];
    keywords: string[];
    isUnmounted: boolean;
  }[] = [
    {
      id: null,
      path: "app",
      names: ["app", "root"],
      keywords: ["app", "root"],
      isUnmounted: false,
    },
    ...components.map((component) => ({
      id: component.id,
      path: component.path,
      names: [component.path, component.name ?? ""],
      // "Switch to" is deliberately excluded so it doesn't match the search;
      // only the component's own name/path does.
      keywords: [component.path, component.name ?? "", "component"],
      isUnmounted: component.state !== "active",
    })),
  ];
  // Filter in JS and render as remote items (paletteFilter never drops remote
  // items), so cmdk doesn't re-score and re-sort — and thus reflow, flashing —
  // these on every keystroke. The per-component shortcuts only make sense while
  // searching; browsing shows just the drill command to keep the list clean.
  const matches = trimmed
    ? sortByMatch(
        targets.filter((target) =>
          target.keywords.some((keyword) => matchesSearch(trimmed, keyword)),
        ),
        trimmed,
        (target) => target.names,
      )
    : [];
  return (
    <Command.Group heading="Components">
      <ActionItem
        value="page:components"
        onSelect={onOpenComponentsPage}
        Icon={CaretSortIcon}
        label="Switch Component…"
        drillIn
      />
      {matches.map((target) => (
        <SwitchToComponentItem
          key={target.id ?? "app"}
          value={`${REMOTE_VALUE_PREFIX}switch-component:${target.id ?? "app"}`}
          path={target.path}
          componentId={target.id}
          keywords={target.keywords}
          isCurrent={selected === target.id}
          isUnmounted={target.isUnmounted}
          onSelect={() => selectComponent(target.id)}
        />
      ))}
    </Command.Group>
  );
}

function ComponentRowBody({
  label,
  isCurrent,
  isUnmounted,
}: {
  label: React.ReactNode;
  isCurrent: boolean;
  isUnmounted: boolean;
}) {
  return (
    <>
      <PuzzlePieceIcon className="text-content-secondary" />
      <span className="min-w-0 truncate">{label}</span>
      <span className="ml-auto flex shrink-0 items-center gap-1.5 text-xs text-content-tertiary">
        {isUnmounted && "Unmounted"}
        {isCurrent && <CurrentBadge />}
      </span>
    </>
  );
}

function SwitchToComponentItem({
  value,
  path,
  componentId,
  keywords,
  isCurrent,
  isUnmounted,
  onSelect,
}: {
  value: string;
  path: string;
  componentId: string | null;
  keywords: string[];
  isCurrent: boolean;
  isUnmounted: boolean;
  onSelect: () => void;
}) {
  const componentHref = useComponentHref();
  useCopyAction(value, { label: "component path", getText: () => path });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      keywords={keywords}
      onSelect={onSelect}
    >
      <ItemPrimary href={componentHref(componentId)}>
        <ComponentRowBody
          label={
            <>
              {'Switch to "'}
              <HighlightedText text={path} />
              {'"'}
            </>
          }
          isCurrent={isCurrent}
          isUnmounted={isUnmounted}
        />
      </ItemPrimary>
    </Command.Item>
  );
}

// The drilled-into "Switch Component" page: the app plus every installed
// component.
export function ComponentsCommands({ onClose }: { onClose: () => void }) {
  const connected = useMaybeConnectedDeployment();
  if (!connected?.deployment) {
    return <LoadingSignal />;
  }
  return (
    <ConvexProvider client={connected.deployment.client}>
      <ComponentsList onClose={onClose} />
    </ConvexProvider>
  );
}

function ComponentsList({ onClose }: { onClose: () => void }) {
  const router = useRouter();
  const components = useComponents();
  const selected =
    typeof router.query.component === "string" ? router.query.component : null;
  const selectComponent = useSelectComponent(onClose);
  const search = useCommandState((state) => state.search);

  if (!components) {
    return <LoadingSignal />;
  }

  return (
    <Command.Group heading="Components">
      <ComponentItem
        value="component:app"
        label="app"
        componentId={null}
        keywords={["app", "root"]}
        isCurrent={selected === null}
        onSelect={() => selectComponent(null)}
      />
      {sortByMatch(components, search, (component) => [
        component.path,
        component.name,
      ]).map((component) => (
        <ComponentItem
          key={component.id}
          value={`component:${component.id}`}
          label={component.path}
          componentId={component.id}
          keywords={[component.path, component.name ?? "", "component"]}
          isCurrent={selected === component.id}
          isUnmounted={component.state !== "active"}
          onSelect={() => selectComponent(component.id)}
        />
      ))}
    </Command.Group>
  );
}

function ComponentItem({
  value,
  label,
  componentId,
  keywords,
  isCurrent,
  isUnmounted = false,
  onSelect,
}: {
  value: string;
  label: string;
  componentId: string | null;
  keywords: string[];
  isCurrent: boolean;
  isUnmounted?: boolean;
  onSelect: () => void;
}) {
  const { trackSelected } = usePaletteAnalytics();
  const track = () => trackSelected("switch-component");
  const componentHref = useComponentHref();
  useCopyAction(value, { label: "component path", getText: () => label });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      keywords={keywords}
      onSelect={() => {
        track();
        onSelect();
      }}
    >
      <ItemPrimary href={componentHref(componentId)} onModifiedClick={track}>
        <ComponentRowBody
          label={<HighlightedText text={label} />}
          isCurrent={isCurrent}
          isUnmounted={isUnmounted}
        />
      </ItemPrimary>
    </Command.Item>
  );
}
