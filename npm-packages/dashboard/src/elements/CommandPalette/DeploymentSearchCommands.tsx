import { Command } from "cmdk";
import { useContext, useMemo } from "react";
import { ConvexProvider, useQuery } from "convex/react";
import { useRouter } from "next/router";
import { CodeIcon, TableIcon } from "@radix-ui/react-icons";
import udfs from "@common/udfs";
import {
  DeploymentInfoContext,
  useMaybeConnectedDeployment,
} from "@common/lib/deploymentContext";
import { getReferencedTableName } from "@common/lib/utils";
import {
  processAnalyzedModuleFunction,
  type ModuleFunction,
} from "@common/lib/functionHelpers";
import type { ComponentId } from "@common/lib/useNents";
import type { UdfType } from "system-udfs/convex/_system/frontend/common";
import { useHttpActionRoute } from "@common/features/functions/lib/useHttpActionRoute";
import { matchesSearch, NavigationDestination } from "./navigation";
import { REMOTE_VALUE_PREFIX } from "./navigation";
import { HighlightedText } from "./items";
import type { DocumentSearchResultItem } from "./DocumentSearchResult";
import {
  documentGroupHeading,
  useDocumentValue,
  ViewDocumentItem,
  type DocumentRef,
} from "./DocumentCommands";
import { useCopyAction } from "./copy";

const MAX_RESULTS = 20;

// Data-plane search within the current deployment: tables, functions,
// components, and documents (by ID).
export function DeploymentSearchCommands({
  search,
  onNavigate,
  onOpenDetail,
}: {
  search: string;
  onNavigate: (to: NavigationDestination) => void;
  // Opens the detail side panel for a document / file / scheduled-job match.
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
}) {
  const connected = useMaybeConnectedDeployment();
  if (!connected?.deployment) {
    return null;
  }
  return (
    <ConvexProvider client={connected.deployment.client}>
      <DeploymentSearchInner
        search={search}
        onNavigate={onNavigate}
        onOpenDetail={onOpenDetail}
      />
    </ConvexProvider>
  );
}

function DeploymentSearchInner({
  search,
  onNavigate,
  onOpenDetail,
}: {
  search: string;
  onNavigate: (to: NavigationDestination) => void;
  onOpenDetail: (detail: DocumentSearchResultItem) => void;
}) {
  const router = useRouter();
  const { deploymentsURI, useIsOperationAllowed } = useContext(
    DeploymentInfoContext,
  );
  const canViewData = useIsOperationAllowed("ViewData");
  const trimmed = search.trim();
  // Fetch lazily: these subscriptions only start once the user types.
  const enabled = canViewData && trimmed.length > 0;

  // Search is scoped to the component the user is currently viewing
  const currentComponent =
    typeof router.query.component === "string" ? router.query.component : null;

  const tableMapping = useQuery(
    udfs.getTableMapping.default,
    enabled ? { componentId: currentComponent } : "skip",
  );
  const rawModules = useQuery(
    udfs.modules.list,
    enabled ? { componentId: currentComponent } : "skip",
  );

  const referencedTableName = getReferencedTableName(tableMapping, trimmed);
  const matched: DocumentRef | null =
    enabled && referencedTableName
      ? {
          tableName: referencedTableName,
          id: trimmed,
          componentId: currentComponent,
        }
      : null;
  const document = useDocumentValue(matched);

  const functions: ModuleFunction[] = useMemo(() => {
    if (!rawModules) {
      return [];
    }
    const result: ModuleFunction[] = [];
    for (const [filePath, module] of rawModules) {
      for (const fn of module.functions) {
        result.push(
          processAnalyzedModuleFunction(
            fn,
            filePath,
            currentComponent as ComponentId,
            null,
          ),
        );
      }
    }
    return result;
  }, [rawModules, currentComponent]);

  if (!enabled) {
    return null;
  }

  const matchingTables = Object.values(tableMapping ?? {})
    .filter((name) => !name.startsWith("_"))
    .filter((name) => matchesSearch(trimmed, name))
    .slice(0, MAX_RESULTS);

  const matchingFunctions = functions
    .filter((fn) => matchesSearch(trimmed, fn.displayName))
    .slice(0, MAX_RESULTS);

  return (
    <>
      {/* A document ID is unique within a component, so the document it decodes
          to is the only match. */}
      {matched && document !== undefined && (
        <Command.Group heading={documentGroupHeading(matched.tableName)}>
          <ViewDocumentItem
            document={matched}
            value={document}
            onOpenDetail={onOpenDetail}
          />
        </Command.Group>
      )}
      {matchingTables.length > 0 && (
        <Command.Group heading="Tables">
          {matchingTables.map((name) => (
            <TableResultItem
              key={name}
              tableName={name}
              componentId={currentComponent}
              deploymentsURI={deploymentsURI}
              onNavigate={onNavigate}
            />
          ))}
        </Command.Group>
      )}
      {matchingFunctions.length > 0 && (
        <Command.Group heading="Functions">
          {matchingFunctions.map((fn) => (
            <FunctionResultItem
              key={`${fn.componentId ?? ""}:${fn.identifier}`}
              fn={fn}
              deploymentsURI={deploymentsURI}
              onNavigate={onNavigate}
            />
          ))}
        </Command.Group>
      )}
    </>
  );
}

function FunctionResultItem({
  fn,
  deploymentsURI,
  onNavigate,
}: {
  fn: ModuleFunction;
  deploymentsURI: string;
  onNavigate: (to: NavigationDestination) => void;
}) {
  const value = `${REMOTE_VALUE_PREFIX}function:${fn.componentId ?? ""}:${fn.identifier}`;
  // An HTTP action's own name is only the route path, so copy the absolute URL
  // the route is served at instead — that's what you'd paste into a client.
  const route = useHttpActionRoute(fn);
  useCopyAction(
    value,
    route?.status === "mounted"
      ? { label: "URL", getText: () => route.url }
      : { label: "function name", getText: () => fn.displayName },
  );
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      onSelect={() =>
        onNavigate({
          pathname: `${deploymentsURI}/functions`,
          query: {
            function: fn.displayName,
            // `component` (the component ID) is the param useNents and the rest
            // of the page key off of.
            ...(fn.componentId ? { component: fn.componentId } : {}),
          },
        })
      }
    >
      <CodeIcon className="text-content-secondary" />
      <span className="truncate font-mono">
        <HighlightedText text={fn.displayName} />
      </span>
      <span className="ml-auto shrink-0 text-xs text-content-tertiary">
        {udfTypeLabel(fn.udfType)}
      </span>
    </Command.Item>
  );
}

function TableResultItem({
  tableName,
  componentId,
  deploymentsURI,
  onNavigate,
}: {
  tableName: string;
  componentId: string | null;
  deploymentsURI: string;
  onNavigate: (to: NavigationDestination) => void;
}) {
  const value = `${REMOTE_VALUE_PREFIX}table:${componentId ?? ""}:${tableName}`;
  useCopyAction(value, { label: "table name", getText: () => tableName });
  return (
    <Command.Item
      value={value}
      className="animate-fadeInFromLoading"
      onSelect={() =>
        onNavigate({
          pathname: `${deploymentsURI}/data`,
          query: {
            table: tableName,
            ...(componentId ? { component: componentId } : {}),
          },
        })
      }
    >
      <TableIcon className="text-content-secondary" />
      <span className="truncate font-mono">
        <HighlightedText text={tableName} />
      </span>
      <span className="ml-auto shrink-0 text-xs text-content-tertiary">
        Table
      </span>
    </Command.Item>
  );
}

function udfTypeLabel(udfType: UdfType): string {
  return udfType === "HttpAction" ? "HTTP Action" : udfType;
}
